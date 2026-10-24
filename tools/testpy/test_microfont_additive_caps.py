#!/usr/bin/env python3
"""Execute the production cap GLSL math in a minimal C++ vector shim."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def block(source, marker):
    start = source.index(marker)
    brace = source.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]

def main():
    vert = (ROOT / 'vendor/microfont/shaders/mask.vert').read_text()
    frag = (ROOT / 'vendor/microfont/shaders/mask.frag').read_text()
    cap = block(frag, 'float end_coverage(')
    growth = block(vert, 'if (p.style.x > 0.0)')
    shim = r'''
#include <algorithm>
#include <cassert>
#include <cmath>
using std::abs;
float clamp(float v,float lo,float hi) {return std::clamp(v,lo,hi);}
struct vec2 {
    float x,y;
    vec2(float x=0,float y=0):x(x),y(y){}
    vec2 operator-(vec2 b) const {return {x-b.x,y-b.y};}
    vec2 operator*(float b) const {return {x*b,y*b};}
    void operator-=(vec2 b) {x-=b.x;y-=b.y;}
};
float dot(vec2 a,vec2 b) {return a.x*b.x+a.y*b.y;}
float length(vec2 a) {return std::sqrt(dot(a,a));}
struct vec3 {vec2 xy;float z;};
struct vec4 {
    float x,y,z,w;vec2 xy,zw;
    vec4(float x=0,float y=0,float z=0,float w=0):x(x),y(y),z(z),w(w),xy(x,y),zw(z,w){}
};
struct {vec4 style,start_point_direction,end_point_direction;} p;
struct {vec2 xy;} gl_FragCoord;
vec2 source_pixel(vec2 at) {return at;}
'''
    tests = r'''
int main() {
    for (float width : {3.5f,7.0f,8.0f,9.0f}) {
        // Production dots use sqrt(2) diameter, quantized to native U11.7.
        float r=std::round(width*std::sqrt(2.0f)*128)/256;
        vec2 at(100,100);
        p.start_point_direction=vec4(100,100,1,0);
        p.end_point_direction=vec4(100,100,-1,0);
        for (float start : {0.0f,1.0f}) for (float end : {0.0f,1.0f}) {
            p.style=vec4(r,0,start,end);
            assert(length(extend({at,1.25f})-vec2(100-r,100))==0);
            assert(length(extend({at,1.75f})-vec2(100+r,100))==0);
            for (float marker : {0.0f,0.5f,1.0f})
                assert(length(extend({at,marker})-at)==0);
            for (int x=-16;x<=16;x++) for (int y=-16;y<=16;y++) {
                vec2 d(x*r/16,y*r/16);
                gl_FragCoord.xy=vec2(100+d.x,100+d.y);
                float side=clamp(r-abs(d.y),0,1);
                float actual=std::min(side,std::min(end_coverage(p.start_point_direction,start),
                                                   end_coverage(p.end_point_direction,end)));
                float expected=((d.x<0 && start!=0) || (d.x>0 && end!=0))
                    ? clamp(r-length(d),0,1) : side;
                assert(abs(actual-expected)<0.00001f);
            }
        }
    }
    for (float width : {7.0f,8.0f,9.0f}) {
        float r=width/2;
        for (int angle=0;angle<360;angle++) {
            float a=angle*3.14159265358979323846f/180;
            vec2 inward(std::cos(a),std::sin(a));
            p.style=vec4(r,0,1,1);
            p.start_point_direction=vec4(100,100,inward.x,inward.y);
            p.end_point_direction=vec4(100,100,-inward.x,-inward.y);
            vec2 at(100,100);
            assert(length(extend({at,0.25f})-(at-inward*r))<0.0001f);
            assert(length(extend({at,0.75f})-(at-inward*(-r)))<0.0001f);
            // Coincident endpoints remain distinct by index. Interior and
            // invisible adjacency vertices retain their original positions.
            for (float marker : {0.0f,0.5f,1.0f})
                assert(length(extend({at,marker})-at)==0);
            p.style.z=0;p.style.w=0;
            assert(length(extend({at,0.25f})-at)==0);
            assert(length(extend({at,0.75f})-at)==0);
            gl_FragCoord.xy=at;
            assert(end_coverage(p.start_point_direction,1)==1);
            gl_FragCoord.xy=at-inward*(r/2);
            assert(end_coverage(p.start_point_direction,1)>0.99f);
            assert(end_coverage(p.start_point_direction,0)==1);
            gl_FragCoord.xy=at-inward*(r+0.25f);
            assert(end_coverage(p.start_point_direction,1)==0);
            gl_FragCoord.xy=at-inward*(r+2);
            assert(end_coverage(p.start_point_direction,1)==1);
        }
    }
}
'''
    code = shim + cap + '\nvec2 extend(vec3 source) {vec2 pixel=source_pixel(source.xy);\n' + growth + '\nreturn pixel;}\n' + tests
    with tempfile.TemporaryDirectory(prefix='microfont-additive-caps-') as tmp:
        tmp = Path(tmp)
        (tmp / 'test.cpp').write_text(code)
        subprocess.run(['c++', '-std=c++17', str(tmp / 'test.cpp'), '-o', str(tmp / 'test')], check=True)
        subprocess.run([str(tmp / 'test')], check=True)
    print('Production cap math: round/square dots, all cap tags, 360 angles and endpoint/index/junction checks passed.')

if __name__ == '__main__':
    main()
