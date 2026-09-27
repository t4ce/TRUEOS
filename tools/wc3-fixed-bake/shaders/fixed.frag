#version 450
layout(location=0) in vec4 primary;
layout(location=1) in vec4 texcoord;
layout(location=2) in float fog_distance;
layout(location=0) out vec4 color;
layout(set=0,binding=0,std430) readonly buffer State { vec4 s[]; };
layout(set=0,binding=3) uniform texture2D image;
layout(set=0,binding=4) uniform sampler image_sampler;
void main() {
    vec4 c=primary;
    int env=int(s[25].x);
    if(env!=0) {
        vec4 t=texture(sampler2D(image,image_sampler),texcoord.xy/texcoord.w);
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
