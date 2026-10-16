"""Keep raw socket progress private and distinct from HTTP/request attribution."""
import json
import unittest

from codex_phase_report import ALLOCATOR_WORK_NUMBERS, NETWORK_PROGRESS_NUMBERS, markdown, parse_log_text


def row(prefix='[blueprint] [info] ', **changes):
    fields = dict.fromkeys(NETWORK_PROGRESS_NUMBERS, 0)
    fields.update(gen=7, current_gen=7, rx_bytes=1000, read_bytes=600, queued=400,
                  socket_queue_peak=250)
    fields.update(changes)
    return prefix + 'codex-progress-net ' + ' '.join(f'{k}={v}' for k, v in fields.items()) + '\n'


class NetworkProgressReportTests(unittest.TestCase):
    def test_only_static_numeric_progress_is_retained(self):
        raw = row().rstrip() + ' endpoint=URL_SECRET payload=PAYLOAD_SECRET\n'
        report = parse_log_text(raw)
        self.assertEqual(len(report['network_progress']), 1)
        observed = report['network_progress'][0]
        self.assertEqual((observed['rx_bytes'], observed['read_bytes'], observed['queued']), (1000, 600, 400))
        self.assertEqual(observed['socket_queue_peak'], 250)
        for output in (json.dumps(report), markdown(report)):
            self.assertNotIn('SECRET', output)

    def test_busy_missing_and_bad_values_do_not_become_zero_samples(self):
        raw = '[blueprint] [info] codex-progress-net registry=busy private=PRIVATE_SECRET\n'
        raw += row(rx_bytes=-1) + row(read_calls=2**64) + row(paused=2) + row(vm=256)
        raw += 'codex-progress-net vm=0 gen=1\n'
        raw += row('output=') + row('[apps] [info] arguments=')
        report = parse_log_text(raw)
        self.assertEqual(report['network_progress'], [dict(line=1, run=None, registry='busy')])
        self.assertNotIn('SECRET', json.dumps(report))

    def test_generations_and_observations_are_preserved_without_joined_deltas(self):
        raw = row(gen=7, current_gen=8, paused=1) + row(gen=8, current_gen=8, rx_bytes=10, read_bytes=0)
        report = parse_log_text(raw)
        self.assertEqual([r['gen'] for r in report['network_progress']], [7, 8])
        self.assertEqual(report['mcp_captures'], [])
        self.assertEqual(report['turns'], [])
        old = dict(report)
        del old['network_progress']
        self.assertIn('Network progress samples:', markdown(old))


class AllocatorWorkReportTests(unittest.TestCase):
    def test_counter_resets_remain_separate_raw_observations(self):
        def work(**changes):
            fields = dict(zip(ALLOCATOR_WORK_NUMBERS, (0, 10, 120, 40, 1)))
            fields.update(changes)
            return '[blueprint] [info] codex-allocator-work ' + ' '.join(f'{k}={v}' for k, v in fields.items()) + '\n'
        raw = work() + work(alloc_calls=2, alloc_nodes=3, alloc_max_nodes=2, alloc_failures=0)
        report = parse_log_text(raw)
        self.assertEqual([r['alloc_nodes'] for r in report['allocator_work']], [120, 3])
        self.assertEqual(report['turns'], [])
        old = dict(report)
        del old['allocator_work']
        self.assertIn('Allocator work samples:', markdown(old))

    def test_private_or_invalid_work_fields_are_never_exported(self):
        raw = 'codex-allocator-work vm=0 alloc_calls=4 alloc_nodes=8 alloc_max_nodes=3 alloc_failures=0 address=ADDRESS_SECRET sql=SQL_SECRET\n'
        raw += 'codex-allocator-work vm=0 alloc_calls=4 alloc_nodes=8 alloc_max_nodes=-1 alloc_failures=0\n'
        raw += 'output=' + raw.splitlines()[0] + '\n'
        report = parse_log_text(raw)
        self.assertEqual(len(report['allocator_work']), 1)
        self.assertNotIn('SECRET', json.dumps(report))


if __name__ == '__main__':
    unittest.main()
