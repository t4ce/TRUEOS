#!/usr/bin/env python3
"""Validate current read-only documentation without connecting to the rig."""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / 'tools/testpy/trueos-doc'


def invoke(*args: str, cwd: Path | None = None, repo: Path | None = None) -> subprocess.CompletedProcess:
    env = dict(os.environ, TRUEOS_REPO=str(repo or ROOT))
    return subprocess.run([str(CLI), *args], cwd=cwd or ROOT, env=env,
                          capture_output=True, text=True, timeout=10)


def run(*args: str, **kwargs) -> dict:
    completed = invoke(*args, **kwargs)
    if completed.returncode:
        raise AssertionError(completed.stderr)
    response = json.loads(completed.stdout)
    assert response['ok']
    return response['data']


class TrueosDocTests(unittest.TestCase):
    def test_context_uses_current_build_access_and_shell(self) -> None:
        data = run()
        self.assertEqual(data['rig_builds']['invocation'], 'cargo rigdev')
        self.assertEqual(data['rig_builds']['release_invocation'], 'cargo rigreal')
        self.assertEqual(data['rig_builds']['build_outputs']['published_runtime'],
                         'tgt/pxe-current/TRUEOS.elf')
        self.assertEqual(data['rig']['ssh_port'], 22)
        self.assertTrue(data['rig']['pty_required'])
        self.assertEqual(data['shell3']['default_mode'], 'CMD')
        self.assertIn('gpu', data['shell3']['commands']['adm'])
        self.assertNotIn('make_iso', data)
        self.assertNotIn('shell2', data)

    def test_rig_topic_uses_ssh_with_optional_existing_identity(self) -> None:
        data = run('topic', 'rig')
        self.assertEqual(data['physical']['host'], '192.168.178.94')
        self.assertEqual(data['connect'], 'ssh -tt -p 22 t4ce@192.168.178.94')
        self.assertNotIn('identity', data['physical'])
        self.assertIn('only when', data['identity'])
        self.assertIn('retired', data['retired'])

    def test_commands_use_current_shell3_name_groups(self) -> None:
        gpu = run('command', 'gpu')
        self.assertEqual(gpu['mode'], 'adm')
        self.assertEqual(gpu['invocation'], 'gpu')
        self.assertNotIn('parameters', gpu)
        self.assertEqual(set(run('command', 'stop')['modes']), {'hv', 'vme'})
        modes = run('commands')['modes']
        self.assertEqual(modes['cmd'], [])  # Runtime AppDB/Aka, not a guessed static list.
        self.assertIn('tui', [entry['name'] for entry in modes['vme']])
        self.assertEqual({entry['name'] for entry in modes['hv']},
                         {'status', 'peer', 'probe', 'start', 'stop', 'pause', 'resume', 'kill'})

    def test_registry_is_read_from_the_selected_repository(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary)
            (repo / 'Cargo.toml').touch()
            (repo / 'src/hv').mkdir(parents=True)
            (repo / 'src/hv/mod.rs').touch()
            source = repo / 'src/shell3/names.rs'
            source.parent.mkdir()
            source.write_text('''
const HV_GROUP_1: [NameEntry; 1] = [
    NameEntry { name: "fixtureprobe", color: RgbaColor::White },
];
''')
            data = run('command', 'fixtureprobe', repo=repo)
            self.assertEqual(data['mode'], 'hv')
            self.assertEqual(data['source'], 'src/shell3/names.rs::HV_GROUP_1')
            self.assertEqual(run('commands', '--mode', 'hv', repo=repo)['modes']['hv'][0]['name'],
                             'fixtureprobe')

    def test_retired_shell2_commands_return_structured_errors(self) -> None:
        completed = invoke('command', 'xhci')
        self.assertEqual(completed.returncode, 2)
        self.assertEqual(completed.stdout, '')
        self.assertFalse(json.loads(completed.stderr)['ok'])
        self.assertEqual(run('topic', 'shell2')['status'], 'retired')

    def test_it_runs_outside_the_repository_by_absolute_path(self) -> None:
        self.assertEqual(run('topic', '§', cwd=Path('/tmp'))['name'], 'headjack')

    def test_search_discovers_current_rig_builds_and_access(self) -> None:
        self.assertEqual(run('search', 'cargo rigdev appdb build')['results'][0]['name'], 'rig-builds')
        self.assertEqual(run('search', 'testrig ssh shell port')['results'][0]['name'], 'rig')

    def test_retired_build_queries_redirect_without_retired_instructions(self) -> None:
        for name in ('make-iso', 'make iso', 'iso', 'redeploy'):
            with self.subTest(name=name):
                data = run('topic', name)
                self.assertEqual(data['name'], 'rig-builds')
                self.assertEqual(data['invocation'], 'cargo rigdev')
                encoded = json.dumps(data)
                self.assertNotIn('make iso', encoded)
                self.assertNotIn('smb://', encoded)
                self.assertNotIn('START_BAREMETAL_LOG', encoded)
        self.assertNotIn('make-iso', run('topics')['topics'])

    def test_build_topic_contains_the_single_database_and_native_boot_paths(self) -> None:
        data = run('topic', 'buildins')
        self.assertEqual(data['build_outputs']['embedded_database'],
                         'tgt/buildins/<dev|release>/app.db.lz4')
        self.assertEqual(data['build_outputs']['uefi'], 'tgt/pxe-current/EFI/BOOT/BOOTX64.EFI')
        self.assertEqual(data['prepare_without_reset'],
                         'bash tools/trueos-boot/prepare.sh --dev')
        self.assertIn('--strip-debug', ' '.join(data['workflow']))
        self.assertIn('automatically reloads', ' '.join(data['workflow']))
        self.assertIn('once', data['workflow'][-1])
        self.assertNotIn('receipt', data.get('build_outputs', {}))
        self.assertEqual(run('topic', 'blueprints')['build_topic'], 'rig-builds')

    def test_logs_keep_the_actual_bld_capture_location(self) -> None:
        data = run('topic', 'logfiles')
        capture = data['captures']['baremetal']
        self.assertEqual(capture['latest'], 'bld/baremetal-logs/LatestOfThree.logs')
        self.assertNotIn('orchestration_log', capture)
        self.assertIn('bld/baremetal-logs', (ROOT / 'tools/baremetal-log-drain.sh').read_text())
        self.assertIn('do not start', data['build_integration'])
        self.assertEqual(data['levels']['order'][0], 'Error')
        self.assertIn('Never expect it in a logfile', data['filter_invariant'])

    def test_release_profile_matches_the_shared_builtin_configuration(self) -> None:
        profile = run('topic', 'rig-builds')['release_profile']
        source = ROOT / profile['config']
        actual = tomllib.loads(source.read_text())['profile']['release']
        self.assertEqual(profile['settings'], actual)
        self.assertEqual(run()['rig_builds']['release_profile'], profile)

    def test_live_update_archive_has_native_members_and_distinct_install_endpoint(self) -> None:
        data = run('topic', 'live-update')
        update = data['live_update']
        self.assertEqual(update['members'],
                         ['TRUEOS.elf', 'EFI/BOOT/BOOTX64.EFI', 'EFI/BOOT/tgl_guc_70.bin'])
        self.assertEqual(data['build_outputs']['live_update_dev'], 'tgt/rigdev.tar.lz4')
        self.assertEqual(data['build_outputs']['live_update_release'], 'tgt/rigreal.tar.lz4')
        self.assertEqual(data['build_outputs']['live_update_current'], 'tgt/TrueOS.tar.lz4')
        self.assertEqual(update['current_link_target'], 'pxe-current/TrueOS.tar.lz4')
        self.assertEqual(update['remote_url'], 'https://trueos.eu/TrueOS.tar.lz4')
        self.assertEqual(update['lan_url'], 'http://192.168.178.111:8080/TrueOS.tar.lz4')
        self.assertIn('https://trueos.eu/TrueOS.lz4', update['online_install'])
        self.assertEqual(run()['rig_builds']['live_update'], update)

    def test_references_resolve_to_the_outsourced_document_tree(self) -> None:
        data = run('topic', 'html')
        self.assertEqual(len(data['documents']), 5)
        for document in data['documents']:
            self.assertTrue(document['path'].startswith('../tools/docs/'))
            self.assertTrue((ROOT / document['path']).is_file(), document['path'])

    def test_trueosfs_http_retains_root_aware_download_contract(self) -> None:
        data = run('topic', 'filesystem-api')
        self.assertEqual(data['discovery']['path'], '/')
        download = next(route for route in data['routes'] if route['path'].startswith('/dl/<root-id>'))
        self.assertFalse(download['mutation'])
        self.assertIn('pic', data['screenshot_pull'][0])
        self.assertIn('do not silently substitute an old file', data['screenshot_pull'][-1])

    def test_webcam_topic_remains_host_only_when_not_registered(self) -> None:
        data = run('topic', 'webcam-frame')
        self.assertEqual(data['package'], 'webcam-frame')
        self.assertEqual(data['availability']['shell3_startable'],
                         bool(data['availability']['registered_selectors']))
        self.assertIn('bld/baremetal-logs/film', data['storage']['explicit_rig_output'])


if __name__ == '__main__':
    unittest.main()
