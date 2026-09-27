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
vec4 fetch_repeat(ivec2 p, ivec2 size, int row) {
    p=(p%size+size)%size;
    return texelFetch(sampler2D(image,image_sampler),ivec2(p.x,p.y+row),0);
}
vec4 sample_level(vec2 uv, int level, bool linear_filter) {
    level=clamp(level,0,14);
    ivec2 base=ivec2(s[30].xy),size=max(base>>level,ivec2(1));
    int row=0;
    for(int i=0;i<level;i++) row+=max(base.y>>i,1);
    vec2 p=fract(uv)*vec2(size);
    if(!linear_filter) return fetch_repeat(ivec2(floor(p)),size,row);
    p-=0.5;
    ivec2 q=ivec2(floor(p)); vec2 f=fract(p);
    return mix(mix(fetch_repeat(q,size,row),fetch_repeat(q+ivec2(1,0),size,row),f.x),
        mix(fetch_repeat(q+ivec2(0,1),size,row),fetch_repeat(q+ivec2(1,1),size,row),f.x),f.y);
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
    vec4 c=primary;
    int env=int(s[25].x);
    if(env!=0) {
        vec4 t=sample_gl(texcoord.xy/texcoord.w);
        bool rgb=s[25].y!=0.0;
        if(env==1) c=vec4(primary.rgb*t.rgb,primary.a*(rgb?1.0:t.a));
        if(env==2) c=vec4(rgb?t.rgb:mix(primary.rgb,t.rgb,t.a),primary.a);
        if(env==3) c=vec4(t.rgb,rgb?primary.a:t.a);
        if(env==4) c=vec4(mix(primary.rgb,s[26].rgb,t.rgb),primary.a*(rgb?1.0:t.a));
    }
    int fog=int(s[23].x);
    if(fog!=0) {
        float z=abs(fog_distance),f;
        if(fog==1) f=(s[23].w-z)/(s[23].w-s[23].z);
        else if(fog==2) f=exp(-s[23].y*z);
        else f=exp(-s[23].y*s[23].y*z*z);
        c.rgb=mix(s[24].rgb,c.rgb,clamp(f,0.0,1.0));
    }
    color=c;
}
