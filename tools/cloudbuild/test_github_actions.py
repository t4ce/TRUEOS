"""Exercise dispatch recovery, provenance admission and private API delivery.

GitHub and compilation are fixtures. These tests never contact GitHub or
represent their sentinel bytes as a bootable TRUEOS image.
"""
from __future__ import annotations

import base64
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import subprocess
import time
import unittest
from unittest.mock import patch
import urllib.error
import zipfile

from build_plan import catalog, resolve_request
from ci import DEPENDENCIES, EXPECTED_RUSTC_COMMIT
from github_actions import ActionsWorker, AppToken, GitHub, read_config
from service import NAMESPACE, Service, Store
from identity import verify_receipt


class FakeGitHub:
    def __init__(self, config):
        self.config = config
        self.calls = []
        self.dispatched = None
        self.lost_response = False
        self.run = None
        self.archive = b''

    def json(self, method, path, body=None):
        self.calls.append((method, path, body))
        if '/commits/' in path:
            return {'sha': self.config['source_commit']}
        if path.endswith('/dispatches'):
            self.dispatched = body
            self.run = {'id': 17, 'display_title': 'Cloudbuild ' + body['inputs']['request_id'],
                        'head_sha': self.config['source_commit'], 'event': 'workflow_dispatch',
                        'path': '.github/workflows/cloudbuild.yml', 'run_attempt': 1,
                        'repository': {'full_name': self.config['repository']},
                        'status': 'queued', 'conclusion': None}
            if self.lost_response:
                raise OSError('response lost after acceptance')
            return {'workflow_run_id': 17}
        if '/runs?' in path:
            return {'workflow_runs': [self.run] if self.run else []}
        if '/artifacts?' in path:
            return {'artifacts': [{'id': 8, 'size_in_bytes': len(self.archive), 'expired': False,
                                  'name': 'trueos-' + json.loads(self.dispatched['inputs']['manifest'])['config_hash'] + '-17',
                                  'digest': 'sha256:' + hashlib.sha256(self.archive).hexdigest()}]}
        if path.endswith('/cancel'):
            return {}
        if path.endswith('/runs/17'):
            return self.run
        raise AssertionError((method, path))

    def download(self, path, destination):
        assert path.endswith('/artifacts/8/zip')
        destination.write_bytes(self.archive)


class ActionsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.now = 1800000000
        self.config = {'repository': 't4ce/TRUEOS', 'workflow': 'cloudbuild.yml', 'ref': 'cloudbuild-approved',
                       'source_commit': '1' * 40,
                       'pins': {name: str(i) * 40 for i, name in enumerate(DEPENDENCIES, start=2)}}
        self.store = Store(self.directory / 'state', clock=lambda: self.now)
        self.service = Service(self.store, self.config['source_commit'], 'https://build.example.org',
                               allow_anonymous=True, backend='github-actions')
        self.manifest = resolve_request({'preset': 'kernel-only'}, self.config['source_commit'])
        self.ticket = self.store.queue_anonymous(self.manifest)
        self.github = FakeGitHub(self.config)
        self.worker = ActionsWorker(self.store, self.config, self.github)

    def job(self):
        return self.service.get_job(self.ticket['job_id'], self.ticket['access_token'])

    def make_archive(self, *, modify=None, extra=None):
        bundle = b'\x04\x22\x4d\x18SENTINEL: not an OS image'
        sources = {'trueos': {'repository': 't4ce/TRUEOS', 'commit': self.config['source_commit']}}
        sources.update({name: {'repository': 't4ce/' + repo, 'commit': self.config['pins'][name]}
                        for name, repo in DEPENDENCIES.items() if name != 'buildins' or self.manifest['buildins']})
        provenance = {'sources': sources, 'config_hash': self.manifest['config_hash'],
                      'workflow': {'repository': self.config['repository'], 'run_id': '17', 'run_attempt': '1'},
                      'rustc_verbose_version': 'commit-hash: ' + EXPECTED_RUSTC_COMMIT,
                      'artifact': {'sha256': hashlib.sha256(bundle).hexdigest()}}
        if modify:
            modify(provenance)
        files = {'TrueOS.tar.lz4': bundle, 'build-manifest.json': json.dumps(self.manifest).encode(),
                 'provenance.json': json.dumps(provenance).encode(), 'Cargo.lock': b'fixture lock',
                 'SHA256SUMS': b'fixture checksums'}
        if extra:
            files.update(extra)
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        self.github.archive = stream.getvalue()
        return provenance

    def complete(self):
        self.github.run.update(status='completed', conclusion='success')
        self.worker.tick()

    def test_dispatch_poll_collect_and_bearer_download_contract(self):
        self.worker.tick()
        self.assertEqual(self.job()['status'], 'building')
        self.assertEqual(self.job()['phase'], 'dispatching')
        inputs = self.github.dispatched['inputs']
        self.assertEqual(set(inputs), {'request_id', 'manifest', 'blueprints_commit', 'toolchain_commit'})
        self.assertNotIn(self.ticket['access_token'], json.dumps(self.github.calls))
        self.assertEqual(json.loads(inputs['manifest']), self.manifest)
        self.worker.tick()
        self.assertEqual(self.job()['phase'], 'waiting-for-runner')
        self.github.run['status'] = 'in_progress'
        self.worker.tick()
        self.assertEqual(self.job()['phase'], 'compiling')
        provenance = self.make_archive()
        self.complete()
        self.assertEqual(self.job()['status'], 'succeeded')
        self.assertEqual(self.job()['provenance'], provenance)
        artifact = self.store.artifact_path(self.ticket['job_id'])
        self.assertEqual(self.job()['artifact']['sha256'], hashlib.sha256(artifact.read_bytes()).hexdigest())
        with self.assertRaises(Exception):
            self.service.get_job(self.ticket['job_id'], 'a' * 43)

    def test_accepted_dispatch_with_lost_response_is_discovered_after_restart_once(self):
        self.github.lost_response = True
        self.worker.tick()
        restarted = ActionsWorker(Store(self.store.directory, clock=lambda: self.now), self.config, self.github)
        restarted.tick()
        self.assertEqual(self.job()['phase'], 'waiting-for-runner')
        self.assertEqual(sum(path.endswith('/dispatches') for _, path, _ in self.github.calls), 1)
        self.make_archive()
        self.github.run.update(status='completed', conclusion='success')
        restarted.tick()
        self.assertEqual(self.job()['status'], 'succeeded')

    def test_keyed_request_receives_a_verified_receipt_and_sends_no_identity_to_github(self):
        user, issuer = self.directory / 'person', self.directory / 'issuer'
        for key in (user, issuer):
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)],
                           check=True, capture_output=True)
        public = user.with_suffix('.pub').read_text()
        fingerprint = self.store.grant(public, 'FIXTURE-PRIVATE-LICENSE', [f['id'] for f in catalog()['features']])
        self.service = Service(self.store, self.config['source_commit'], 'https://build.example.org',
                               issuer_key=issuer, backend='github-actions')
        # Complete the previous anonymous fixture so the signed job is next.
        with self.store.connect() as db:
            db.execute("DELETE FROM jobs")
        challenge = self.service.post('/api/challenges', {'request': {'preset': 'kernel-only'}, 'public_key': public})
        signed = subprocess.run(['ssh-keygen', '-Y', 'sign', '-f', str(user), '-n', NAMESPACE],
                                input=challenge['challenge'].encode(), capture_output=True, check=True).stdout.decode()
        self.ticket = self.service.post('/api/builds', {'challenge_id': challenge['challenge_id'], 'signature': signed})
        self.worker.tick()
        transmitted = json.dumps(self.github.calls)
        for private in (fingerprint, 'FIXTURE-PRIVATE-LICENSE', self.ticket['access_token'], public.strip()):
            self.assertNotIn(private, transmitted)
        self.make_archive()
        self.complete()
        result = self.job()
        self.assertEqual(result['status'], 'succeeded')
        self.assertTrue(verify_receipt(result['receipt'], issuer.with_suffix('.pub').read_text()))
        self.assertEqual(result['receipt']['receipt']['subject_key_fingerprint'], fingerprint)
        self.assertEqual(result['receipt']['receipt']['artifact_sha256'], result['artifact']['sha256'])

    def test_pending_run_prevents_second_dispatch(self):
        self.store.queue_anonymous(self.manifest)
        self.worker.tick()
        self.worker.tick()
        self.assertEqual(sum(path.endswith('/dispatches') for _, path, _ in self.github.calls), 1)

    def test_source_pin_mismatch_never_dispatches(self):
        self.config['source_commit'] = 'a' * 40
        self.worker.tick()
        self.assertIsNone(self.github.dispatched)
        self.assertEqual(self.job()['status'], 'failed')

    def test_run_binding_rejects_another_revision_workflow_repository_or_rerun(self):
        self.worker.tick()
        for key, value in [('head_sha', 'a' * 40), ('path', '.github/workflows/release.yml'),
                           ('display_title', 'different'), ('repository', {'full_name': 'other/repo'}),
                           ('run_attempt', 2), ('event', 'push')]:
            with self.subTest(key=key):
                original = self.github.run[key]
                self.github.run[key] = value
                with self.store.connect() as db:
                    row = dict(db.execute('SELECT j.*, a.* FROM jobs j JOIN actions_jobs a ON a.job_id=j.id').fetchone())
                with self.assertRaises(ValueError):
                    self.worker.poll(row)
                self.github.run[key] = original

    def test_bad_dependency_provenance_rejects_artifact(self):
        self.worker.tick()
        self.make_archive(modify=lambda p: p['sources']['blueprints'].update(commit='a' * 40))
        self.complete()
        self.assertEqual(self.job()['status'], 'failed')
        self.assertFalse(self.store.artifact_path(self.ticket['job_id']).exists())

    def test_bad_bundle_hash_rejects_artifact(self):
        self.worker.tick()
        self.make_archive(modify=lambda p: p['artifact'].update(sha256='0' * 64))
        self.complete()
        self.assertEqual(self.job()['status'], 'failed')

    def test_archive_path_traversal_is_rejected_without_extraction(self):
        self.worker.tick()
        self.make_archive(extra={'../../escape': b'escape'})
        self.complete()
        self.assertEqual(self.job()['status'], 'failed')
        self.assertFalse((self.store.directory / 'escape').exists())

    def test_expired_deadline_cancels_known_run(self):
        self.worker.tick()
        self.now += 10801
        self.worker.tick()
        self.assertEqual(self.job()['status'], 'failed')
        self.assertTrue(any(path.endswith('/cancel') for _, path, _ in self.github.calls))

    def test_failed_actions_run_is_never_a_successful_build(self):
        self.worker.tick()
        self.github.run.update(status='completed', conclusion='failure')
        self.worker.tick()
        self.assertEqual(self.job()['status'], 'failed')

    def test_unknown_dispatch_is_not_retried(self):
        self.github.lost_response = True
        self.worker.tick()
        self.github.run = None
        self.now += 601
        self.worker.tick()
        self.assertEqual(self.job()['status'], 'failed')
        self.assertEqual(sum(path.endswith('/dispatches') for _, path, _ in self.github.calls), 1)

    def test_public_artifact_policy_is_exposed_by_catalog(self):
        self.assertTrue(self.service.info()['capabilities']['artifacts_public'])

    def test_operator_config_validates_pins_and_rejects_extra_execution_settings(self):
        path = self.directory / 'config.json'
        path.write_text(json.dumps(self.config))
        self.assertEqual(read_config(path), self.config)
        bad = copy.deepcopy(self.config)
        bad['command'] = 'echo arbitrary'
        path.write_text(json.dumps(bad))
        with self.assertRaises(ValueError):
            read_config(path)
        bad = copy.deepcopy(self.config)
        bad['pins']['toolchain'] = 'main'
        path.write_text(json.dumps(bad))
        with self.assertRaises(ValueError):
            read_config(path)

    def test_app_token_is_signed_and_cached_without_sending_private_key(self):
        key, public, signature = (self.directory / name for name in ('app.pem', 'app.pub', 'signature'))
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:2048',
                        '-out', str(key)], capture_output=True, check=True)
        subprocess.run(['openssl', 'pkey', '-in', str(key), '-pubout', '-out', str(public)],
                       capture_output=True, check=True)
        expires = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(time.time() + 3600))
        def issue(client, method, path, body):
            self.assertEqual((method, path), ('POST', '/app/installations/123/access_tokens'))
            self.assertEqual(body, {'permissions': {'actions': 'write', 'contents': 'read'}})
            jwt = client.token()
            header, payload, sig = jwt.split('.')
            decoded = lambda value: base64.urlsafe_b64decode(value + '=' * (-len(value) % 4))
            self.assertEqual(json.loads(decoded(header))['alg'], 'RS256')
            claims = json.loads(decoded(payload))
            self.assertEqual(claims['iss'], 'Iv1fixture')
            self.assertLessEqual(claims['exp'] - claims['iat'], 600)
            signature.write_bytes(decoded(sig))
            subprocess.run(['openssl', 'dgst', '-sha256', '-verify', str(public), '-signature', str(signature)],
                           input=(header + '.' + payload).encode(), check=True, capture_output=True)
            return {'token': 'FIXTURE-INSTALLATION-TOKEN', 'expires_at': expires}
        with patch('github_actions.GitHub.json', autospec=True, side_effect=issue) as request:
            token = AppToken('Iv1fixture', '123', key)
            self.assertEqual(token(), 'FIXTURE-INSTALLATION-TOKEN')
            self.assertEqual(token(), 'FIXTURE-INSTALLATION-TOKEN')
            self.assertEqual(request.call_count, 1)

    def test_storage_download_does_not_forward_github_authorization(self):
        class Opener:
            def __init__(self): self.requests = []
            def open(self, request, timeout):
                self.requests.append(request)
                if len(self.requests) == 1:
                    raise urllib.error.HTTPError(request.full_url, 302, 'redirect',
                        {'Location': 'https://fixture.blob.core.windows.net/result?signature=secret'}, io.BytesIO())
                return io.BytesIO(b'fixture archive')
        github = GitHub(lambda: 'PRIVATE-TOKEN')
        github.opener = Opener()
        destination = self.directory / 'download'
        github.download('/repos/t4ce/TRUEOS/actions/artifacts/8/zip', destination)
        first, second = github.opener.requests
        self.assertEqual(first.get_header('Authorization'), 'Bearer PRIVATE-TOKEN')
        self.assertIsNone(second.get_header('Authorization'))
        self.assertEqual(destination.read_bytes(), b'fixture archive')


if __name__ == '__main__':
    unittest.main()
