"""Persistent GitHub Actions backend for the private Cloudbuild queue.

Only operator configuration selects repositories, refs and dependencies. No
identity, bearer ticket or issuer key is sent to GitHub. Dispatch attempts are
recorded BEFORE network I/O and never automatically repeated after uncertainty.
"""
from __future__ import annotations

import base64
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

from build_plan import validate_manifest
from ci import DEPENDENCIES, EXPECTED_RUSTC_COMMIT
from identity import canonical_json
from service import APIError, Store, digest_file, strict_object

MAX_DOWNLOAD = 512 * 1024 * 1024
MAX_JSON = 2 * 1024 * 1024
SHA = re.compile(r"[a-f0-9]{40}\Z")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def read_config(path):
    config = json.loads(Path(path).read_text(), object_pairs_hook=strict_object)
    required = {"repository", "workflow", "ref", "source_commit", "pins"}
    if not isinstance(config, dict) or set(config) != required:
        raise ValueError("Actions configuration requires repository, workflow, ref, source_commit and pins")
    if not isinstance(config['repository'], str) or not re.fullmatch(r"[\w.-]+/[\w.-]+", config['repository'], re.ASCII):
        raise ValueError("Invalid operator repository")
    if not isinstance(config['workflow'], str) or not re.fullmatch(r"[\w.-]+\.ya?ml", config['workflow'], re.ASCII):
        raise ValueError("Invalid workflow filename")
    if not isinstance(config['ref'], str) or not re.fullmatch(r"[A-Za-z0-9_./-]{1,200}", config['ref']):
        raise ValueError("Invalid operator workflow ref")
    if not isinstance(config['source_commit'], str) or not SHA.fullmatch(config['source_commit']):
        raise ValueError("source_commit must be an immutable full SHA")
    if (not isinstance(config['pins'], dict) or set(config['pins']) != set(DEPENDENCIES)
            or any(not isinstance(v, str) or not SHA.fullmatch(v) for v in config['pins'].values())):
        raise ValueError("Pin buildins, blueprints and toolchain to full SHAs")
    return config


class AppToken:
    """Mint short-lived installation tokens; the App key stays outside runners."""
    def __init__(self, client_id, installation_id, private_key):
        if not re.fullmatch(r"[A-Za-z0-9_]+", client_id) or not re.fullmatch(r"[0-9]+", installation_id):
            raise ValueError("Invalid GitHub App client or installation ID")
        self.client_id, self.installation_id = client_id, installation_id
        self.key = Path(private_key)
        if not self.key.is_file():
            raise ValueError("GitHub App private key is missing")
        self.token, self.until = None, 0

    def __call__(self):
        if self.token and time.time() < self.until:
            return self.token
        def encode(value):
            return base64.urlsafe_b64encode(value).rstrip(b'=')
        now = int(time.time())
        header = encode(canonical_json({"alg": "RS256", "typ": "JWT"}))
        payload = encode(canonical_json({"iat": now - 60, "exp": now + 540, "iss": self.client_id}))
        signing = header + b'.' + payload
        signature = subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(self.key)],
                                   input=signing, capture_output=True, check=True, timeout=10).stdout
        jwt = (signing + b'.' + encode(signature)).decode('ascii')
        response = GitHub(lambda: jwt).json('POST', f'/app/installations/{self.installation_id}/access_tokens',
                                           {"permissions": {"actions": "write", "contents": "read"}})
        self.token = response['token']
        self.until = datetime.fromisoformat(response['expires_at'].replace('Z', '+00:00')).timestamp() - 120
        return self.token


class GitHub:
    def __init__(self, token):
        self.token = token
        # Explicit redirects: never send the API Authorization header to storage.
        self.opener = urllib.request.build_opener(NoRedirect())

    def request(self, method, path, body=None):
        request = urllib.request.Request('https://api.github.com' + path, method=method,
            data=canonical_json(body) if body is not None else None,
            headers={'Authorization': 'Bearer ' + self.token(), 'Accept': 'application/vnd.github+json',
                     'Content-Type': 'application/json', 'X-GitHub-Api-Version': '2026-03-10',
                     'User-Agent': 'TRUEOS-Cloudbuild'})
        return self.opener.open(request, timeout=30)

    def json(self, method, path, body=None):
        with self.request(method, path, body) as response:
            raw = response.read(MAX_JSON + 1)
        if len(raw) > MAX_JSON:
            raise ValueError('GitHub API response exceeds size limit')
        return json.loads(raw, object_pairs_hook=strict_object) if raw else {}

    def download(self, path, destination):
        try:
            self.request('GET', path).close()
            raise ValueError('Expected an artifact download redirect')
        except urllib.error.HTTPError as error:
            if error.code != 302:
                raise
            location = error.headers.get('Location', '')
            error.close()
        parsed = urllib.parse.urlsplit(location)
        if (parsed.scheme != 'https' or parsed.username or parsed.password or parsed.port not in (None, 443)
                or not parsed.hostname or not parsed.hostname.endswith(('.blob.core.windows.net', '.githubusercontent.com'))):
            raise ValueError('Unexpected artifact storage destination')
        # A fresh request carries no GitHub authorization. Disallow further redirects.
        with self.opener.open(urllib.request.Request(location), timeout=30) as response, destination.open('wb') as output:
            size = 0
            while chunk := response.read(1024 * 1024):
                size += len(chunk)
                if size > MAX_DOWNLOAD:
                    raise ValueError('Artifact download exceeds size limit')
                output.write(chunk)


class ActionsWorker:
    def __init__(self, store: Store, config, github, *, timeout=10800):
        self.store, self.config, self.github = store, config, github
        self.timeout = timeout
        self.base = '/repos/' + config['repository']
        self.workflow = self.base + '/actions/workflows/' + config['workflow']
        with store.connect() as db:
            db.execute('''CREATE TABLE IF NOT EXISTS actions_jobs (
                job_id TEXT PRIMARY KEY, configuration TEXT NOT NULL,
                dispatched_at INTEGER NOT NULL, run_id INTEGER)''')

    def claim(self):
        with self.store.connect(transaction=True) as db:
            # One remote build per bridge, with a durable pending record.
            if db.execute("SELECT 1 FROM actions_jobs a JOIN jobs j ON a.job_id=j.id WHERE j.status='building'").fetchone():
                return None
            row = db.execute("SELECT * FROM jobs WHERE status='queued' ORDER BY created_at,id LIMIT 1").fetchone()
            if row is None:
                return None
            now = int(self.store.clock())
            db.execute("UPDATE jobs SET status='building', started_at=? WHERE id=?", (now, row['id']))
            db.execute('INSERT INTO actions_jobs VALUES (?,?,?,NULL)',
                       (row['id'], json.dumps(self.config, sort_keys=True), now))
            return dict(row)

    def dispatch(self, job):
        manifest = validate_manifest(json.loads(job['manifest']), self.config['source_commit'])
        self.authorize(job, manifest)
        ref = urllib.parse.quote(self.config['ref'], safe='')
        revision = self.github.json('GET', self.base + '/commits/' + ref)
        if revision['sha'] != self.config['source_commit']:
            raise ValueError('Operator workflow ref moved; provision a new approved source revision')
        inputs = {'manifest': json.dumps(manifest, separators=(',', ':')), 'request_id': job['id']}
        inputs.update({name + '_commit': sha for name, sha in self.config['pins'].items()
                       if name != 'buildins' or manifest['buildins']})
        result = self.github.json('POST', self.workflow + '/dispatches', {'ref': self.config['ref'], 'inputs': inputs})
        run_id = result.get('workflow_run_id')
        if run_id is not None:
            if type(run_id) is not int or run_id < 1:
                raise ValueError('Invalid dispatched run ID')
            with self.store.connect(transaction=True) as db:
                db.execute('UPDATE actions_jobs SET run_id=? WHERE job_id=?', (run_id, job['id']))

    def authorize(self, job, manifest):
        if job['fingerprint']:
            with self.store.connect() as db:
                self.store.authorize(db, job['fingerprint'], manifest, int(self.store.clock()), job['license_id'])

    def discover(self, job):
        since = datetime.fromtimestamp(job['dispatched_at'] - 60, timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        query = urllib.parse.urlencode({'event': 'workflow_dispatch', 'head_sha': self.config['source_commit'],
                                        'created': '>=' + since, 'per_page': 100})
        matches = []
        for page in range(1, 11):
            data = self.github.json('GET', self.workflow + '/runs?' + query + f'&page={page}')
            runs = data['workflow_runs']
            matches += [r['id'] for r in runs if r.get('display_title') == 'Cloudbuild ' + job['id']]
            if len(runs) < 100:
                break
        if len(matches) > 1:
            raise ValueError('Multiple runs matched one dispatch; operator reconciliation required')
        if matches:
            with self.store.connect(transaction=True) as db:
                db.execute('UPDATE actions_jobs SET run_id=? WHERE job_id=?', (matches[0], job['id']))
            return matches[0]
        return None

    def poll(self, job):
        if json.loads(job['configuration']) != self.config:
            raise ValueError('Actions configuration changed while a job was pending; restore its configuration')
        run_id = job['run_id'] or self.discover(job)
        elapsed = self.store.clock() - job['dispatched_at']
        if run_id is None:
            if elapsed > 600:
                raise ValueError('Dispatch outcome is unknown; operator must inspect Actions before requesting another build')
            return
        run = self.github.json('GET', self.base + f'/actions/runs/{run_id}')
        if (run.get('head_sha') != self.config['source_commit'] or run.get('event') != 'workflow_dispatch'
                or run.get('path') != '.github/workflows/' + self.config['workflow']
                or run.get('display_title') != 'Cloudbuild ' + job['id']
                or run.get('repository', {}).get('full_name', '').lower() != self.config['repository'].lower()
                or run.get('run_attempt') != 1):
            raise ValueError('Actions run does not match the approved request and source')
        if elapsed > self.timeout:
            if run['status'] != 'completed':
                self.github.json('POST', self.base + f'/actions/runs/{run_id}/cancel')
            raise ValueError('Cloud build exceeded its deadline')
        if run['status'] != 'completed':
            return
        if run['conclusion'] != 'success':
            raise ValueError('GitHub Actions build ' + str(run['conclusion']))
        manifest = validate_manifest(json.loads(job['manifest']), self.config['source_commit'])
        self.authorize(job, manifest)
        name = f"trueos-{manifest['config_hash']}-{run_id}"
        artifacts = self.github.json('GET', self.base + f'/actions/runs/{run_id}/artifacts?per_page=100')['artifacts']
        matching = [a for a in artifacts if a['name'] == name and not a['expired']]
        if len(matching) != 1:
            raise ValueError('Expected one completed build artifact')
        artifact = matching[0]
        if type(artifact['id']) is not int or not 0 < artifact['size_in_bytes'] <= MAX_DOWNLOAD:
            raise ValueError('Invalid Actions artifact metadata')
        destination = self.store.artifact_path(job['id'])
        destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=destination.parent) as directory:
            archive = Path(directory) / 'artifact.zip'
            self.github.download(self.base + f"/actions/artifacts/{artifact['id']}/zip", archive)
            digest = artifact.get('digest', '')
            if digest != 'sha256:' + digest_file(archive):
                raise ValueError('Actions artifact digest mismatch')
            provenance = self.unpack(archive, Path(directory), manifest, run_id)
            shutil.copyfile(Path(directory) / 'TrueOS.tar.lz4', destination.with_suffix('.partial'))
            destination.with_suffix('.partial').chmod(0o600)
            destination.with_suffix('.partial').replace(destination)
            for filename in ('build-manifest.json', 'provenance.json'):
                shutil.copyfile(Path(directory) / filename, destination.parent / filename)
        self.store.finish_build(job['id'], digest_file(destination), destination.stat().st_size, provenance)

    def unpack(self, archive, directory, manifest, run_id):
        expected = {'TrueOS.tar.lz4', 'build-manifest.json', 'provenance.json', 'Cargo.lock', 'SHA256SUMS'}
        with zipfile.ZipFile(archive) as zipped:
            members = zipped.infolist()
            if (len(members) != len(expected) or {m.filename for m in members} != expected
                    or sum(m.file_size for m in members) > MAX_DOWNLOAD):
                raise ValueError('Unexpected artifact archive contents')
            for member in members:
                if member.filename != 'TrueOS.tar.lz4' and member.file_size > MAX_JSON:
                    raise ValueError('Artifact metadata exceeds size limit')
                # Copy bytes by fixed filename; no archive paths or symlinks are extracted.
                with zipped.open(member) as source, (directory / member.filename).open('wb') as output:
                    shutil.copyfileobj(source, output)
        read = lambda name: json.loads((directory / name).read_text(), object_pairs_hook=strict_object)
        if read('build-manifest.json') != manifest:
            raise ValueError('Downloaded artifact belongs to a different manifest')
        provenance = read('provenance.json')
        sources = {'trueos': {'repository': 't4ce/TRUEOS', 'commit': manifest['source_commit']}}
        sources.update({name: {'repository': 't4ce/' + repo, 'commit': self.config['pins'][name]}
                        for name, repo in DEPENDENCIES.items() if name != 'buildins' or manifest['buildins']})
        workflow = provenance.get('workflow', {})
        if (provenance.get('sources') != sources or provenance.get('config_hash') != manifest['config_hash']
                or workflow.get('repository', '').lower() != self.config['repository'].lower()
                or workflow.get('run_id') != str(run_id) or workflow.get('run_attempt') != '1'
                or f'commit-hash: {EXPECTED_RUSTC_COMMIT}' not in provenance.get('rustc_verbose_version', '').splitlines()):
            raise ValueError('Build provenance does not match approved inputs')
        bundle = directory / 'TrueOS.tar.lz4'
        with bundle.open('rb') as stream:
            if stream.read(4) != b'\x04\x22\x4d\x18':
                raise ValueError('Downloaded build is not an LZ4 frame')
        if provenance.get('artifact', {}).get('sha256') != digest_file(bundle):
            raise ValueError('Build digest does not match provenance')
        return provenance

    def tick(self):
        with self.store.connect() as db:
            pending = [dict(row) for row in db.execute('''SELECT j.*, a.configuration, a.dispatched_at, a.run_id
                FROM actions_jobs a JOIN jobs j ON a.job_id=j.id WHERE j.status='building' ''')]
        for job in pending:
            try:
                self.poll(job)
            except APIError:
                self.store.fail(job['id'], 'The build grant is no longer valid.')
            except (ValueError, KeyError, zipfile.BadZipFile):
                self.store.fail(job['id'], 'Actions result could not be verified. The operator must inspect the build.')
            except (OSError, urllib.error.URLError):
                if self.store.clock() - job['dispatched_at'] > self.timeout:
                    self.store.fail(job['id'], 'Actions backend unavailable beyond the build deadline; operator inspection required.')
        job = self.claim()
        if job:
            try:
                self.dispatch(job)
            except (ValueError, KeyError, APIError):
                self.store.fail(job['id'], 'Build admission failed. The operator must check source pins and authorization.')
            except (OSError, urllib.error.URLError, subprocess.SubprocessError):
                # It may have reached GitHub. Discover on the next tick; never resend.
                pass


def work_forever(store, config, github, *, once=False):
    with (store.directory / 'actions-worker.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError('An Actions worker is already running for this queue')
        worker = ActionsWorker(store, config, github)
        while True:
            worker.tick()
            if once:
                return
            time.sleep(10)
