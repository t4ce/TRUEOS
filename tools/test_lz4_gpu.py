#!/usr/bin/env python3
"""Execute the production shader as native C++ under ASan/UBSan against liblz4.

Only OpenCL address-space annotations and invocation IDs are adapted. The
compression/decompression algorithm itself is the checked-in shader source.
This proves memory safety/interoperability on the host, not GPU throughput.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    shader = (ROOT / 'crates/trueos-shader/gpgpu/kernels/lz4_blocks.clcpp').read_text()
    shader = shader.replace('#include "include/trueos_clcpp.hpp"', '''
#include <algorithm>
#include <cstdint>
using uint = uint32_t;
using uchar = unsigned char;
using std::min;
#define __global
#define __kernel
#define TRUEOS_REQD_SUB_GROUP_SIZE_16
static uint invocation;
static uint get_global_id(uint) { return invocation; }
''')
    harness = r'''
extern "C" int LZ4_compressBound(int);
extern "C" int LZ4_compress_default(const char*, char*, int, int);
extern "C" int LZ4_decompress_safe(const char*, char*, int, int);
#include <cassert>
#include <cstdio>
#include <random>
#include <vector>
using Bytes = std::vector<uchar>;
static Bytes transform(const Bytes& input, uint capacity, bool encode, bool* accepted = nullptr) {
    Bytes output(capacity);
    uint descriptor[] = {0, uint(input.size()), 0, capacity, 0, 1};
    std::vector<uint> hashes(4096);
    invocation = 0;
    lz4_blocks(input.data(), output.data(), descriptor, hashes.data(), 1, encode ? 0 : 1);
    if (accepted) *accepted = descriptor[5] == 0;
    else assert(descriptor[5] == 0);
    assert(descriptor[4] <= capacity);
    output.resize(descriptor[4]); return output;
}
int main() {
    std::mt19937 random(20260924);
    for (uint n : {0u,1u,4u,5u,12u,13u,15u,19u,255u,256u,4095u,4096u,65535u,65536u}) {
        for (uint mode = 0; mode < 5; ++mode) {
            Bytes input(n);
            for (uint i = 0; i < n; ++i) input[i] = mode == 0 ? 0 : mode == 1 ? i % 7 : mode == 2 ? random() : mode == 3 ? (i / 31) % 17 : random() % 8;
            auto compressed = transform(input, LZ4_compressBound(n), true);
            Bytes restored(n);
            auto read = LZ4_decompress_safe(reinterpret_cast<const char*>(compressed.data()), reinterpret_cast<char*>(restored.data()), compressed.size(), n);
            assert(read == int(n) && restored == input);
            Bytes reference(LZ4_compressBound(n));
            auto size = LZ4_compress_default(reinterpret_cast<const char*>(input.data()), reinterpret_cast<char*>(reference.data()), n, reference.size());
            assert(size > 0); reference.resize(size);
            assert(transform(reference, n, false) == input);
            assert(transform(compressed, n, false) == input);
        }
    }
    // Malformed-input differential fuzzing with exact-sized, ASan-visible
    // allocations. Every reference-accepted block must decode identically.
    for (uint trial = 0; trial < 20000; ++trial) {
        Bytes input(random() % 512); for (auto& b : input) b = random();
        uint cap = random() % 8192;
        Bytes reference(cap);
        int size = LZ4_decompress_safe(reinterpret_cast<const char*>(input.data()), reinterpret_cast<char*>(reference.data()), input.size(), cap);
        bool accepted;
        auto decoded = transform(input, cap, false, &accepted);
        if (size >= 0) { reference.resize(size); assert(accepted && decoded == reference); }
    }
    for (const Bytes& malformed : {Bytes{}, Bytes{0,0,0}, Bytes{0x10,'a',0,0}, Bytes{0xf0}, Bytes{0x10}, Bytes{0,1,0}}) {
        bool accepted; transform(malformed, 64, false, &accepted); assert(!accepted);
    }
    // Distinct lanes use disjoint hash tables even in a partially filled wave.
    Bytes input(17*4096, 42), output(17*4140);
    std::vector<uint> descriptors(17*6), hashes(2*4096);
    for (uint i = 0; i < 17; ++i) {
        auto d = &descriptors[i*6]; d[0]=i*4096; d[1]=4096; d[2]=i*4140; d[3]=4140;
    }
    for (invocation = 0; invocation < 32; ++invocation) lz4_blocks(input.data(), output.data(), descriptors.data(), hashes.data(), 17, 0);
    for (uint i = 0; i < 17; ++i) {
        auto d = &descriptors[i*6]; assert(d[5] == 0);
        Bytes restored(4096);
        assert(LZ4_decompress_safe(reinterpret_cast<const char*>(&output[d[2]]), reinterpret_cast<char*>(restored.data()), d[4], 4096) == 4096);
        assert(restored == Bytes(4096,42));
    }
    puts("LZ4 shader: reference interoperability, 20000 malformed cases, lane isolation passed (ASan/UBSan)");
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-lz4-shader-') as directory:
        directory = Path(directory)
        source = directory / 'test.cpp'; source.write_text(shader + harness)
        executable = directory / 'test'
        subprocess.run(['g++', '-std=c++17', '-O1', '-g', '-fsanitize=address,undefined', str(source), '-l:liblz4.so.1', '-o', str(executable)], check=True)
        subprocess.run([str(executable)], check=True)

if __name__ == '__main__': main()
