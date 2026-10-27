#!/usr/bin/env python3
"""Round-trip tiny native boot bundles without compiling or deploying."""
import ctypes
import ctypes.util
import importlib.util
import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('boot_bundle', ROOT / 'tools/trueos-boot/bundle.py')
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class BootBundle(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        library = ctypes.util.find_library('lz4')
        if library is None:
            raise unittest.SkipTest('liblz4 is required by the bundle packager')
        cls.lz4 = ctypes.CDLL(library)
        cls.lz4.LZ4F_createDecompressionContext.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_uint]
        cls.lz4.LZ4F_createDecompressionContext.restype = ctypes.c_size_t
        cls.lz4.LZ4F_freeDecompressionContext.argtypes = [ctypes.c_void_p]
        cls.lz4.LZ4F_freeDecompressionContext.restype = ctypes.c_size_t
        cls.lz4.LZ4F_decompress.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t),
                                         ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p]
        cls.lz4.LZ4F_decompress.restype = ctypes.c_size_t
        cls.lz4.LZ4F_isError.argtypes = [ctypes.c_size_t]
        cls.lz4.LZ4F_isError.restype = ctypes.c_uint

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.inputs = {
            'TRUEOS.elf': b'\x7fELF' + b'kernel code and embedded app.db\n' * 4096,
            'EFI/BOOT/BOOTX64.EFI': b'MZnative UEFI loader',
            'EFI/BOOT/tgl_guc_70.bin': b'GuC firmware',
        }
        for name, contents in self.inputs.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)

    def decode(self, path):
        data = path.read_bytes()
        self.assertEqual(data[:4], b'\x04\x22\x4d\x18')
        context = ctypes.c_void_p()
        result = self.lz4.LZ4F_createDecompressionContext(ctypes.byref(context), 100)
        self.assertFalse(self.lz4.LZ4F_isError(result))
        try:
            source = ctypes.create_string_buffer(data)
            source_size = ctypes.c_size_t(len(data))
            destination = ctypes.create_string_buffer(1024 * 1024)
            destination_size = ctypes.c_size_t(len(destination))
            result = self.lz4.LZ4F_decompress(context, destination, ctypes.byref(destination_size),
                                               source, ctypes.byref(source_size), None)
            self.assertFalse(self.lz4.LZ4F_isError(result))
            self.assertEqual(result, 0, 'the bundle must contain a complete LZ4 frame')
            self.assertEqual(source_size.value, len(data), 'do not append frames or trailing bytes')
            return destination.raw[:destination_size.value]
        finally:
            self.lz4.LZ4F_freeDecompressionContext(context)

    def test_archive_round_trip_has_exact_regular_boot_members(self):
        output = bundle.create_bundle(self.root)
        self.assertEqual(output, self.root / 'TrueOS.tar.lz4')
        decoded = self.decode(output)
        self.assertEqual(decoded[257:265], b'ustar\x0000')
        with tarfile.open(fileobj=io.BytesIO(decoded), mode='r:') as archive:
            members = archive.getmembers()
            self.assertEqual(set(member.name for member in members), set(self.inputs))
            self.assertEqual(len(members), len(self.inputs))
            for member in members:
                self.assertTrue(member.isfile())
                self.assertFalse(member.pax_headers)
                self.assertEqual(archive.extractfile(member).read(), self.inputs[member.name])

    def test_bundle_is_deterministic_despite_source_metadata_changes(self):
        output = bundle.create_bundle(self.root)
        expected = output.read_bytes()
        for name in self.inputs:
            path = self.root / name
            path.chmod(0o600)
            os.utime(path, (1_234_567_890, 1_234_567_890))
        self.assertEqual(bundle.create_bundle(self.root).read_bytes(), expected)

    def test_missing_required_input_preserves_the_previous_archive(self):
        output = bundle.create_bundle(self.root)
        expected = output.read_bytes()
        (self.root / 'EFI/BOOT/tgl_guc_70.bin').unlink()
        with self.assertRaises((ValueError, OSError)):
            bundle.create_bundle(self.root)
        self.assertEqual(output.read_bytes(), expected)

    def test_stream_write_or_finish_failure_preserves_archive_and_cleans_temporary_file(self):
        output = bundle.create_bundle(self.root)
        expected = output.read_bytes()
        original_write = bundle.Lz4Frame.write

        def fail_after_write(frame, data):
            original_write(frame, data)
            raise OSError('injected stream write failure')

        def fail_finish(_):
            raise OSError('injected finish failure')

        for method, replacement in (('write', fail_after_write),
                                    ('finish', fail_finish)):
            with self.subTest(method=method), patch.object(bundle.Lz4Frame, method, replacement):
                with self.assertRaisesRegex(OSError, 'injected'):
                    bundle.create_bundle(self.root)
            self.assertEqual(output.read_bytes(), expected)
            self.assertEqual(list(self.root.glob('.TrueOS.tar.lz4.*')), [])

    def test_member_and_archive_size_limits_preserve_previous_output(self):
        output = bundle.create_bundle(self.root)
        expected = output.read_bytes()
        for constant, limit, message in (('MAX_FILE_BYTES', 32, 'member'),
                                         ('MAX_TAR_BYTES', 1024, 'tar'),
                                         ('MAX_DOWNLOAD_BYTES', 1, 'download')):
            with self.subTest(limit=constant), patch.object(bundle, constant, limit):
                with self.assertRaisesRegex(ValueError, message):
                    bundle.create_bundle(self.root)
            self.assertEqual(output.read_bytes(), expected)
            self.assertEqual(list(self.root.glob('.TrueOS.tar.lz4.*')), [])


if __name__ == '__main__':
    unittest.main()
