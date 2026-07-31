#!/usr/bin/env python3

import hashlib
import importlib.util
import struct
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("compile_misaki_lexicon.py")
SPEC = importlib.util.spec_from_file_location("compile_misaki_lexicon", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COMPILER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPILER)


class CompilerTests(unittest.TestCase):
    def test_gold_default_overrides_and_variants_are_preserved(self) -> None:
        entries, variants, overlap = COMPILER.merge_dictionaries(
            {"alpha": "silver-a", "same": "silver"},
            {
                "beta": "gold-b",
                "same": {"DEFAULT": "gold", "NOUN": "gold-n", "VERB": None},
            },
        )
        self.assertEqual(overlap, 1)
        self.assertEqual(
            entries,
            [("alpha", "silver-a"), ("beta", "gold-b"), ("same", "gold")],
        )
        self.assertEqual(variants, [("same", "NOUN", "gold-n")])

    def test_artifact_is_deterministic_and_self_sealed(self) -> None:
        entries = [("alpha", "a"), ("beta", "b")]
        variants = [("beta", "NOUN", "B")]
        first = COMPILER.build_artifact(entries, variants)
        second = COMPILER.build_artifact(entries, variants)
        self.assertEqual(first, second)
        self.assertEqual(first[:8], COMPILER.MAGIC)
        self.assertEqual(struct.unpack_from("<I", first, 16)[0], 2)
        self.assertEqual(struct.unpack_from("<I", first, 20)[0], 1)
        stored = first[COMPILER.DIGEST_OFFSET : COMPILER.DIGEST_END]
        unsealed = bytearray(first)
        unsealed[COMPILER.DIGEST_OFFSET : COMPILER.DIGEST_END] = bytes(32)
        self.assertEqual(stored, hashlib.sha256(unsealed).digest())

    def test_duplicate_json_keys_are_rejected(self) -> None:
        with self.assertRaises(COMPILER.CompileError):
            COMPILER.decode_dictionary(b'{"same":"a","same":"b"}', "fixture")


if __name__ == "__main__":
    unittest.main()
