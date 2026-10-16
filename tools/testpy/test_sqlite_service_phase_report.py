#!/usr/bin/env python3
"""Privacy and compatibility checks for numeric SQLite service measurements."""
import json
import unittest

from codex_phase_report import SQLITE_SERVICE_NUMBERS, markdown, parse_log_text


def sample(prefix='', **overrides):
    fields = {key: index for index, key in enumerate(SQLITE_SERVICE_NUMBERS, 1)}
    fields.update(operation='execute', phase='complete', completed='true')
    fields.update(overrides)
    return prefix + 'sqlite-service-timing ' + ' '.join(f'{key}={value}' for key, value in fields.items()) + '\n'


class SqliteServiceReportTests(unittest.TestCase):
    def test_host_and_native_records_keep_only_numeric_and_static_fields(self):
        raw = sample('[hv] [info] blueprint vm1: ', skipped=7).rstrip() + ' sql="SQL_SECRET" endpoint="URL_SECRET"\n'
        raw += sample(connection_id=0, request_id=0)
        report = parse_log_text(raw)
        self.assertEqual(len(report['sqlite_service']), 2)
        native, host = report['sqlite_service']
        self.assertEqual((native['vm'], native['skipped'], native['operation']), (1, 7, 'execute'))
        self.assertNotIn('vm', host)
        self.assertEqual((host['connection_id'], host['request_id']), (0, 0))
        self.assertEqual(native['before_bytes'], 8)
        self.assertNotIn('SECRET', json.dumps(report))

    def test_invalid_values_and_embedded_payload_markers_are_ignored(self):
        raw = sample(operation='PRIVATE_OPERATION') + sample(phase='PRIVATE_PHASE')
        raw += sample(elapsed_us=-1) + sample(reply_bytes=2**64) + sample(completed='PRIVATE_STATUS')
        raw += sample('output=') + sample('[apps] [info] prompt=')
        self.assertEqual(parse_log_text(raw)['sqlite_service'], [])

    def test_samples_are_not_automatically_joined_to_client_or_added(self):
        raw = sample('[hv] [info] blueprint vm1: ', elapsed_us=20)
        raw += sample('[hv] [info] blueprint vm2: ', elapsed_us=40)
        report = parse_log_text(raw)
        self.assertEqual([record['elapsed_us'] for record in report['sqlite_service']], [20, 40])
        self.assertEqual(report['sqlx_rpc'], [])
        self.assertEqual(report['turns'], [])
        old_report = dict(report)
        del old_report['sqlite_service']
        self.assertIn('SQLite service timing samples:', markdown(old_report))


if __name__ == '__main__':
    unittest.main()
