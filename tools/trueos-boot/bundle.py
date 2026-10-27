#!/usr/bin/env python3
"""Pack one prepared native boot generation as a deterministic tar/LZ4 frame."""
import argparse
import ctypes
import ctypes.util
import os
from pathlib import Path
import tarfile
import tempfile


FILES = ("TRUEOS.elf", "EFI/BOOT/BOOTX64.EFI", "EFI/BOOT/tgl_guc_70.bin")
ARCHIVE_NAME = "TrueOS.tar.lz4"
MAX_FILE_BYTES = 256 * 1024 * 1024
MAX_TAR_BYTES = 512 * 1024 * 1024
MAX_DOWNLOAD_BYTES = 128 * 1024 * 1024


class Lz4Frame:
    """Streaming frame writer using the same host liblz4 as builtin packaging."""

    def __init__(self, output):
        library = ctypes.util.find_library("lz4")
        if library is None:
            raise ValueError("live-update packaging requires the host liblz4 library")
        self.lib = ctypes.CDLL(library)
        self.lib.LZ4F_isError.argtypes = [ctypes.c_size_t]
        self.lib.LZ4F_isError.restype = ctypes.c_uint
        self.lib.LZ4F_getErrorName.argtypes = [ctypes.c_size_t]
        self.lib.LZ4F_getErrorName.restype = ctypes.c_char_p
        self.lib.LZ4F_createCompressionContext.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_uint]
        self.lib.LZ4F_createCompressionContext.restype = ctypes.c_size_t
        self.lib.LZ4F_freeCompressionContext.argtypes = [ctypes.c_void_p]
        self.lib.LZ4F_freeCompressionContext.restype = ctypes.c_size_t
        self.lib.LZ4F_compressBound.argtypes = [ctypes.c_size_t, ctypes.c_void_p]
        self.lib.LZ4F_compressBound.restype = ctypes.c_size_t
        self.lib.LZ4F_compressBegin.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
        self.lib.LZ4F_compressBegin.restype = ctypes.c_size_t
        self.lib.LZ4F_compressUpdate.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                             ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
        self.lib.LZ4F_compressUpdate.restype = ctypes.c_size_t
        self.lib.LZ4F_compressEnd.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
        self.lib.LZ4F_compressEnd.restype = ctypes.c_size_t
        self.output = output
        self.context = ctypes.c_void_p()
        self.check(self.lib.LZ4F_createCompressionContext(ctypes.byref(self.context), 100))
        try:
            header = ctypes.create_string_buffer(19)
            size = self.check(self.lib.LZ4F_compressBegin(self.context, header, len(header), None))
            self.output.write(header.raw[:size])
        except BaseException:
            self.close()
            raise

    def check(self, result):
        if self.lib.LZ4F_isError(result):
            raise ValueError(self.lib.LZ4F_getErrorName(result).decode())
        return result

    def write(self, data):
        capacity = self.check(self.lib.LZ4F_compressBound(len(data), None))
        buffer = ctypes.create_string_buffer(capacity)
        size = self.check(self.lib.LZ4F_compressUpdate(self.context, buffer, capacity, data, len(data), None))
        self.output.write(buffer.raw[:size])
        return len(data)

    def finish(self):
        capacity = self.check(self.lib.LZ4F_compressBound(0, None))
        buffer = ctypes.create_string_buffer(capacity)
        size = self.check(self.lib.LZ4F_compressEnd(self.context, buffer, capacity, None))
        self.output.write(buffer.raw[:size])

    def close(self):
        if self.context:
            self.lib.LZ4F_freeCompressionContext(self.context)
            self.context = ctypes.c_void_p()


def create_bundle(directory: Path) -> Path:
    directory = Path(directory)
    sizes = {}
    for name in FILES:
        source = directory / name
        size = source.stat().st_size
        if not source.is_file() or not 0 < size <= MAX_FILE_BYTES:
            raise ValueError(f"invalid or oversized update member: {name}")
        sizes[name] = size
    for name, magic in ((FILES[0], b"\x7fELF"), (FILES[1], b"MZ")):
        with (directory / name).open("rb") as stream:
            if stream.read(len(magic)) != magic:
                raise ValueError(f"invalid update member magic: {name}")
    tar_bytes = sum(512 + ((size + 511) // 512) * 512 for size in sizes.values()) + 1024
    if ((tar_bytes + 10239) // 10240) * 10240 > MAX_TAR_BYTES:
        raise ValueError("update tar exceeds the live-update decode limit")

    destination = directory / ARCHIVE_NAME
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=directory, prefix=f".{ARCHIVE_NAME}.", delete=False) as output:
            temporary = Path(output.name)
            frame = Lz4Frame(output)
            try:
                with tarfile.open(fileobj=frame, mode="w|", format=tarfile.USTAR_FORMAT,
                                  bufsize=1024 * 1024) as archive:
                    for name in FILES:
                        member = tarfile.TarInfo(name)
                        member.size, member.mode, member.mtime = sizes[name], 0o644, 0
                        with (directory / name).open("rb") as source:
                            archive.addfile(member, source)
                frame.finish()
            finally:
                frame.close()
            if output.tell() > MAX_DOWNLOAD_BYTES:
                raise ValueError("compressed update exceeds the live-update download limit")
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(destination)
        return destination
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="prepared native boot directory")
    args = parser.parse_args()
    try:
        archive = create_bundle(args.directory)
    except (OSError, ValueError, tarfile.TarError) as error:
        parser.exit(1, f"live-update bundle: {error}\n")
    print(f"Live-update archive ready: {archive} ({archive.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
