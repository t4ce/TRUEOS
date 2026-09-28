#version 450
layout(location=0) in vec4 primary;
layout(location=1) in vec4 texcoord;
layout(location=2) in float fog_distance;
layout(location=0) out vec4 color;
layout(set=0,binding=0,std430) readonly buffer State { vec4 s[]; };
layout(set=0,binding=3) uniform texture2D image;
layout(set=0,binding=4) uniform sampler image_sampler;
// Mips are stored as unfiltered authored texels in vertical atlas rows.
// Explicit fetches prevent filtering into a neighbouring mip or row padding.
vec4 fetch_wrapped(ivec2 p, ivec2 size, int row) {
    ivec2 mode=ivec2(s[25].zw);
    if((mode.x==1 && (p.x<0 || p.x>=size.x)) ||
       (mode.y==1 && (p.y<0 || p.y>=size.y))) return vec4(0.0);
    p=ivec2(mode.x==0 ? (p.x%size.x+size.x)%size.x : clamp(p.x,0,size.x-1),
        mode.y==0 ? (p.y%size.y+size.y)%size.y : clamp(p.y,0,size.y-1));
    return texelFetch(sampler2D(image,image_sampler),ivec2(p.x,p.y+row),0);
}
vec4 sample_level(vec2 uv, int level, bool linear_filter) {
    level=clamp(level,0,14);
    ivec2 base=ivec2(s[30].xy),size=max(base>>level,ivec2(1));
    int row=0;
    for(int i=0;i<level;i++) row+=max(base.y>>i,1);
    ivec2 wrap=ivec2(s[25].zw);
    uv=vec2(wrap.x==0 ? fract(uv.x) : clamp(uv.x,0.0,1.0),
        wrap.y==0 ? fract(uv.y) : clamp(uv.y,0.0,1.0));
    vec2 p=uv*vec2(size);
    if(!linear_filter) return fetch_wrapped(ivec2(floor(p)),size,row);
    p-=0.5;
    ivec2 q=ivec2(floor(p)); vec2 f=fract(p);
    return mix(mix(fetch_wrapped(q,size,row),fetch_wrapped(q+ivec2(1,0),size,row),f.x),
        mix(fetch_wrapped(q+ivec2(0,1),size,row),fetch_wrapped(q+ivec2(1,1),size,row),f.x),f.y);
}
vec4 sample_gl(vec2 uv) {
    vec2 dx=dFdx(uv)*s[30].xy,dy=dFdy(uv)*s[30].xy;
    float lod=0.5*log2(max(max(dot(dx,dx),dot(dy,dy)),1e-30));
    int mode=int(s[30].z); bool mag_linear=s[30].w!=0.0;
    float crossover=(mag_linear && (mode==2||mode==4)) ? 0.5 : 0.0;
    if(lod<=crossover) return sample_level(uv,0,mag_linear);
    bool min_linear=(mode==1||mode==3||mode==5);
    if(mode<2) return sample_level(uv,0,min_linear);
    lod=clamp(lod,0.0,s[31].x);
    if(mode<4) return sample_level(uv,max(0,int(ceil(lod-0.5))),min_linear);
    int lo=int(floor(lod)),hi=min(lo+1,int(s[31].x));
    return mix(sample_level(uv,lo,min_linear),sample_level(uv,hi,min_linear),fract(lod));
}
void main() {
    gl_FragDepth=s[90].x!=0.0 ?
        clamp(s[90].z+gl_FragCoord.z*(s[90].w-s[90].z)+texcoord.z,0.0,1.0) :
        gl_FragCoord.z;
    vec4 c=primary;
    int env=int(s[25].x);
    if(env!=0) {
        vec4 t=sample_gl(texcoord.xy/texcoord.w);
        if(s[90].x!=0.0) {
            int fmt=int(s[90].y); // Alpha, Luminance, LA, RGB, RGBA, Intensity.
            bool alpha=fmt==0||fmt==2||fmt==4||fmt==5;
            bool luminance=fmt==1||fmt==2||fmt==5;
            vec3 tc=luminance ? t.xxx : t.rgb;
            float sampled_alpha=fmt==5 ? t.r : t.a;
            if(env==1) { // MODULATE
                if(fmt==0) c=vec4(primary.rgb,primary.a*t.a);
                else c=vec4(primary.rgb*tc,primary.a*(alpha?sampled_alpha:1.0));
            }
            if(env==2) { // DECAL, only RGB/RGBA admitted by the CPU bridge.
                c=fmt==3 ? vec4(t.rgb,primary.a) :
                    vec4(mix(primary.rgb,t.rgb,t.a),primary.a);
            }
            if(env==3) { // REPLACE
                if(fmt==0) c=vec4(primary.rgb,t.a);
                else if(fmt==1) c=vec4(t.xxx,primary.a);
                else if(fmt==2) c=vec4(t.xxx,t.a);
                else if(fmt==3) c=vec4(t.rgb,primary.a);
                else if(fmt==4) c=t;
                else c=vec4(t.xxx,sampled_alpha);
            }
            if(env==4) { // BLEND
                c=fmt==0 ? vec4(primary.rgb,primary.a*t.a) :
                    vec4(mix(primary.rgb,s[26].rgb,tc),primary.a*(alpha?t.a:1.0));
            }
        } else {
        bool rgb=s[25].y!=0.0;
        if(env==1) c=vec4(primary.rgb*t.rgb,primary.a*(rgb?1.0:t.a));
        if(env==2) c=vec4(rgb?t.rgb:mix(primary.rgb,t.rgb,t.a),primary.a);
        if(env==3) c=vec4(t.rgb,rgb?primary.a:t.a);
        if(env==4) c=vec4(mix(primary.rgb,s[26].rgb,t.rgb),primary.a*(rgb?1.0:t.a));
        }
    }
    int fog=int(s[23].x);
    if(fog!=0) {
        float z=abs(fog_distance),f;
        if(fog==1) f=(s[23].w-z)/(s[23].w-s[23].z);
        else if(fog==2) f=exp(-s[23].y*z);
        else f=exp(-s[23].y*s[23].y*z*z);
        c.rgb=mix(s[24].rgb,c.rgb,clamp(f,0.0,1.0));
    }
    if(s[31].y!=0.0) {
        int func=int(s[31].z);
        float ref=s[31].w;
        bool pass=func==7 ||
            (func==1 && c.a<ref) ||
            (func==2 && c.a==ref) ||
            (func==3 && c.a<=ref) ||
            (func==4 && c.a>ref) ||
            (func==5 && c.a!=ref) ||
            (func==6 && c.a>=ref);
        if(!pass) discard;
    }
    color=c;
}
