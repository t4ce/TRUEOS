#!/usr/bin/env python3
"""Exercise the release workflow's shell steps without a build or GitHub access."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/release.yml'
COMMIT = 'c95ddd528619a27155f33cd931e7a1b7e0cd4ba1'


def run_block(name):
    """Read a named run block directly; the fixture needs no YAML dependency."""
    lines = WORKFLOW.read_text().splitlines()
    start = lines.index(f'      - name: {name}') + 1
    end = next((index for index in range(start, len(lines))
                if lines[index].startswith('      - name: ')), len(lines))
    script = lines.index('        run: |', start, end) + 1
    return textwrap.dedent('\n'.join(lines[script:end])) + '\n'


class ReleaseWorkflow(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        (self.root / 'tools').mkdir()
        (self.root / 'tools/cnt').write_text('224\n')
        self.output = self.root / 'output'
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                        TEST_ROOT=str(self.root), TEST_COMMIT=COMMIT,
                        GITHUB_OUTPUT=str(self.output),
                        GITHUB_SERVER_URL='https://github.com',
                        GITHUB_REPOSITORY='t4ce/TRUEOS',
                        GH_TOKEN='test-only', GH_REPO='t4ce/TRUEOS')
        self.stub('git', '''
printf '%s\\0' "$@" >> "$TEST_ROOT/git-args"
case "$1" in
    ls-remote)
        if [[ ${TEST_GIT_STATUS:-0} != 0 ]]; then exit "$TEST_GIT_STATUS"; fi
        printf '%s' "${TEST_EXISTING_TAG:-}"
        ;;
    rev-parse) printf '%s\\n' "$TEST_COMMIT" ;;
    *) exit 99 ;;
esac
''')
        self.stub('gh', '''
printf '%s\\0' "$@" >> "$TEST_ROOT/gh-args"
case "$1 $2" in
    'release view') exit "${TEST_RELEASE_VIEW_STATUS:-1}" ;;
    'release create') exit "${TEST_RELEASE_CREATE_STATUS:-0}" ;;
    *) exit 99 ;;
esac
''')

    def stub(self, name, script):
        path = self.bin / name
        path.write_text('#!/usr/bin/env bash\nset -eu\n' + script)
        path.chmod(0o755)

    def step(self, name, **env):
        # GitHub's explicit bash shell uses these failure/pipe settings.
        return subprocess.run(['bash', '--noprofile', '--norc', '-e', '-o',
                               'pipefail', '-c', run_block(name)],
                              cwd=self.root, env=dict(self.env, **env),
                              capture_output=True, text=True, timeout=10)

    def arguments(self, command):
        path = self.root / f'{command}-args'
        return path.read_bytes().decode().split('\0')[:-1] if path.exists() else []

    def metadata(self):
        return dict(line.split('=', 1) for line in self.output.read_text().splitlines())

    def assert_no_metadata(self):
        self.assertFalse(self.output.exists())

    def test_counter_sets_version_tag_asset_and_exact_source_commit(self):
        result = self.step('Read release counter')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.metadata(), {
            'version': '0.0.224', 'tag': 'v0.0.224',
            'asset': 'TrueOS-0.0.224.lz4', 'commit': COMMIT,
        })
        self.assertEqual(self.arguments('git'), [
            'ls-remote', '--tags', 'origin', 'refs/tags/v0.0.224',
            'rev-parse', 'HEAD',
        ])
        self.assertEqual(self.arguments('gh'), ['release', 'view', 'v0.0.224'])
        self.assertEqual((self.root / 'tools/cnt').read_text(), '224\n')

    def test_malformed_counter_stops_before_network_checks(self):
        for counter in ('', '0\n', '-1\n', '0224\n', '224oops\n',
                        '224 \n', '1\n2\n', '$(touch injected)\n'):
            with self.subTest(counter=counter):
                (self.root / 'tools/cnt').write_text(counter)
                result = self.step('Read release counter')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('tools/cnt must contain a positive integer', result.stdout)
                self.assert_no_metadata()
                self.assertEqual(self.arguments('git'), [])
                self.assertEqual(self.arguments('gh'), [])
                self.assertFalse((self.root / 'injected').exists())

    def test_missing_counter_fails(self):
        (self.root / 'tools/cnt').unlink()
        result = self.step('Read release counter')
        self.assertNotEqual(result.returncode, 0)
        self.assert_no_metadata()
        self.assertEqual(self.arguments('git'), [])

    def test_existing_tag_stops_before_build_metadata(self):
        result = self.step('Read release counter',
                           TEST_EXISTING_TAG=f'{COMMIT}\trefs/tags/v0.0.224\n')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Commit a new tools/cnt', result.stdout)
        self.assert_no_metadata()
        self.assertEqual(self.arguments('gh'), [])
        self.assertNotIn('rev-parse', self.arguments('git'))

    def test_existing_release_stops_before_build_metadata(self):
        result = self.step('Read release counter', TEST_RELEASE_VIEW_STATUS='0')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Commit a new tools/cnt', result.stdout)
        self.assert_no_metadata()
        self.assertEqual(self.arguments('gh'), ['release', 'view', 'v0.0.224'])
        self.assertNotIn('rev-parse', self.arguments('git'))

    def test_failed_remote_tag_check_is_not_treated_as_a_new_version(self):
        result = self.step('Read release counter', TEST_GIT_STATUS='23')
        self.assertEqual(result.returncode, 23)
        self.assert_no_metadata()
        self.assertEqual(self.arguments('gh'), [])

    def test_notes_and_publish_use_only_the_archive_checksum_and_source_commit(self):
        result = self.step('Read release counter')
        self.assertEqual(result.returncode, 0, result.stderr)
        metadata = self.metadata()
        release_env = dict(RELEASE_VERSION=metadata['version'],
                           RELEASE_TAG=metadata['tag'],
                           RELEASE_ASSET=metadata['asset'],
                           SOURCE_COMMIT=metadata['commit'])
        target = self.root / 'tgt'
        generation = target / 'rig-images/release.test'
        generation.mkdir(parents=True)
        archive = b'the already-built release tar/LZ4 archive\x00\xff'
        (generation / 'TrueOS.tar.lz4').write_bytes(archive)
        (target / 'rigreal.tar.lz4').symlink_to('rig-images/release.test/TrueOS.tar.lz4')
        # Existing unrelated files must never become release attachments.
        for name in ('SHA256SUMS', 'source.tar.gz', 'old.trueos-sig.json'):
            (target / name).write_text('unrelated')

        result = self.step('Prepare the download and release notes', **release_env)
        self.assertEqual(result.returncode, 0, result.stderr)
        asset = target / metadata['asset']
        self.assertFalse(asset.is_symlink())
        self.assertEqual(asset.read_bytes(), archive)
        digest = hashlib.sha256(archive).hexdigest()
        self.assertEqual((target / 'release-notes.md').read_text(),
                         '## Download\n\n'
                         '[Download TrueOS-0.0.224.lz4]('
                         'https://github.com/t4ce/TRUEOS/releases/download/'
                         'v0.0.224/TrueOS-0.0.224.lz4)\n\n'
                         '| item | sha256 |\n| --- | --- |\n'
                         f'| release bundle | `{digest}` |\n\n'
                         f'Source commit: `{COMMIT}`\n')

        result = self.step('Publish the release', **release_env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.arguments('gh'), [
            'release', 'view', 'v0.0.224',
            'release', 'create', 'v0.0.224', 'tgt/TrueOS-0.0.224.lz4',
            '--target', COMMIT, '--title', 'TRUEOS 0.0.224',
            '--notes-file', 'tgt/release-notes.md',
        ])

    def test_publish_failure_is_reported(self):
        result = self.step('Publish the release', RELEASE_VERSION='0.0.224',
                           RELEASE_TAG='v0.0.224', RELEASE_ASSET='TrueOS-0.0.224.lz4',
                           SOURCE_COMMIT=COMMIT, TEST_RELEASE_CREATE_STATUS='17')
        self.assertEqual(result.returncode, 17)


if __name__ == '__main__':
    unittest.main()
