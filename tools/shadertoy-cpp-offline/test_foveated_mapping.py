#!/usr/bin/env python3
"""Run the production radial coordinate functions on CPU using Clang vectors.

No alternate warp implementation: these are the same functions in the packaged
C++ for OpenCL kernel. The host supplies only vector types and math builtins.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def main():
    clang = shutil.which("clang++") or ROOT / "bld/shadertoy-cpp-toolchain/root/usr/lib/llvm-21/bin/clang++"
    with tempfile.TemporaryDirectory(prefix="shadertoy-focus-") as temp:
        source = Path(temp) / "test.cpp"
        source.write_text(HARNESS.replace("@HELPERS@", str(HERE / "foveated_coordinates.clcpp")))
        exe = Path(temp) / "test"
        subprocess.run([str(clang), "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror", str(source), "-o", str(exe)], check=True)
        subprocess.run([str(exe)], check=True)


HARNESS = r'''
#include <algorithm>
#include <cassert>
#include <cmath>
#include <cstdio>
using float2 = float __attribute__((ext_vector_type(2)));
using float4 = float __attribute__((ext_vector_type(4)));
float length(float2 v) { return std::sqrt(v.x*v.x+v.y*v.y); }
float clamp(float x,float a,float b) { return std::clamp(x,a,b); }
#include "@HELPERS@"
int main() {
    float max_error=0;
    for(float boost : {1.f,1.25f,1.5f,2.f}) {
        float4 focus={1478.245f,925.554f,514.446f,boost};
        float previous=-1;
        for(int i=0;i<=20000;++i) {
            float r=focus.z*i/10000.f;
            float2 p={focus.x+r,focus.y};
            float2 q=st_focus_to_sample(p,focus);
            assert(q.x>=previous); previous=q.x;
            float err=length(st_focus_to_output(q,focus)-p);
            max_error=std::max(err,max_error); assert(err<.002f);
            if(r>=focus.z || boost==1.f) assert(length(q-p)<.001f);
        }
        for(int y=0;y<=1440;y+=3) for(int x=0;x<=2560;x+=3) {
            float2 p={(float)x,(float)y}, q=st_focus_to_sample(p,focus);
            assert(q.x>=0 && q.x<=2560 && q.y>=0 && q.y<=1440);
            assert(length(st_focus_to_output(q,focus)-p)<.002f);
        }
        const float epsilon=.25f;
        float2 p={focus.x,focus.y}, step={epsilon,0};
        float center_pitch=(st_focus_to_sample(p+step,focus).x-st_focus_to_sample(p,focus).x)/epsilon;
        assert(std::abs(center_pitch-boost)<.004f);
        // Position and slope meet identity at the disk boundary, not a ring jump.
        float2 edge={focus.x+focus.z,focus.y};
        float slope=(st_focus_to_sample(edge+step,focus).x-st_focus_to_sample(edge-step,focus).x)/(2*epsilon);
        assert(std::abs(slope-1)<.003f);
    }
    std::printf("production radial map: monotone, bounded, smooth boundary; max inverse error %.6f pixels\n",max_error);
}
'''

if __name__ == "__main__":
    main()
