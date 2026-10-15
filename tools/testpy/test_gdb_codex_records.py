#!/usr/bin/env python3
"""Synthetic decoder bounds/privacy checks; never connects to a VM."""
import unittest
from gdb_codex_records import FIELDS, snapshot


class Decoder(unittest.TestCase):
    def setUp(self):
        self.base = 0xffffffff84000000
        self.array = 0xffff800100001000
        self.record = 0xffff800100002000
        self.memory = {}
        self.reads = []
        self.put(self.base + 8, 1)
        self.put(self.base + 16, self.array)
        self.put(self.base + 24, 1)
        self.put(self.array, self.record)
        self.put(self.record + 0x10, 27)
        self.put(self.record + 0x70, 3)
        self.put(self.record + 0x88, 0xffff800100003000)
        self.put(self.record + 0x90, 60000)
    def put(self, address, value):
        for index, byte in enumerate(value.to_bytes(8, 'little')):
            self.memory[address + index] = byte
    def read(self, address, length):
        self.reads.append((address, length))
        return bytes(self.memory.get(address + index, 0) for index in range(length))
    def test_numeric_whitelist_and_no_identity_or_payload_read(self):
        result = snapshot(self.read, self.base, 0)
        self.assertEqual(result['records'][0]['job'], 27)
        self.assertEqual(result['records'][0]['timeout_ms'], 60000)
        self.assertEqual(result['records'][0]['phase'], 3)
        permitted = {(self.record + offset, 8) for offset in FIELDS.values()}
        actual = {read for read in self.reads if self.record <= read[0] < self.record + 192}
        self.assertEqual(actual, permitted)
    def test_busy_roster_is_never_dereferenced(self):
        self.put(self.base, 1)
        self.assertEqual(snapshot(self.read, self.base, 0)['roster'], 'busy')
        self.assertEqual(self.reads, [(self.base, 1)])
    def test_bad_length_and_pointer_rejected(self):
        self.put(self.base + 24, 2)
        with self.assertRaises(ValueError): snapshot(self.read, self.base, 0)
        self.put(self.base + 24, 1)
        self.put(self.array, 17)
        with self.assertRaises(ValueError): snapshot(self.read, self.base, 0)
    def test_invalid_phase_and_vm_rejected(self):
        self.put(self.record + 0x70, 99)
        with self.assertRaises(ValueError): snapshot(self.read, self.base, 0)
        with self.assertRaises(ValueError): snapshot(self.read, self.base, 64)
    def test_limit_bounds_reads(self):
        self.put(self.base + 8, 2)
        self.put(self.base + 24, 2)
        result = snapshot(self.read, self.base, 0, limit=1)
        self.assertTrue(result['truncated'])
        self.assertEqual(len(result['records']), 1)
        self.assertNotIn((self.array + 8, 8), self.reads)


if __name__ == '__main__': unittest.main()
