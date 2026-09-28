#version 450
layout(location=0) in vec4 position;
layout(location=1) in vec4 normal;
layout(location=2) in vec4 color;
layout(location=3) in vec4 uv;
layout(set=0,binding=0,std430) readonly buffer State { vec4 s[]; };
layout(location=0) out vec4 primary;
layout(location=1) out vec4 texcoord;
layout(location=2) out float fog_distance;
vec3 safe_normalize(vec3 v) { float d=length(v); return d>0.0 ? v/d : vec3(0); }
float safe_pow(float x, float exponent) { return exponent==0.0 ? 1.0 : (x<=0.0 ? 0.0 : pow(x,exponent)); }
void main() {
    // The XP bridge can finish transform, lighting and clipping on the CPU.
    // normal.x then carries its eye-space fog distance; the other attributes
    // are already in the shader's interpolation domain.
    if(s[90].x != 0.0) {
        float near_depth=s[90].z, far_depth=s[90].w;
        float ranged_z=(far_depth-near_depth)*position.z+
            (far_depth+near_depth+2.0*normal.y-1.0)*position.w;
        gl_Position=vec4(position.xy*s[27].xy+position.w*s[27].zw,
            (ranged_z+position.w)*0.5,position.w);
        texcoord=uv;
        fog_distance=normal.x;
        primary=clamp(color,0.0,1.0);
        return;
    }
    vec4 eye=mat4(s[0],s[1],s[2],s[3])*position;
    vec4 clip=mat4(s[4],s[5],s[6],s[7])*eye;
    gl_Position=vec4(clip.xy*s[27].xy+clip.w*s[27].zw,
        (clip.z+clip.w)*0.5,clip.w);
    texcoord=mat4(s[8],s[9],s[10],s[11])*uv;
    fog_distance=abs(eye.z);
    primary=clamp(color,0.0,1.0);
    if(s[21].y!=0.0) {
        vec3 n=(mat4(s[12],s[13],s[14],s[15])*vec4(normal.xyz,0)).xyz;
        if(s[21].z!=0.0) n=safe_normalize(n);
        vec4 a=s[17],d=s[18],p=s[19],e=s[20];
        int cm=int(s[22].x);
        if(cm==1||cm==5) a=primary;
        if(cm==2||cm==5) d=primary;
        if(cm==3) p=primary;
        if(cm==4) e=primary;
        vec3 v=eye.xyz/eye.w;
        vec3 viewer=s[21].w!=0.0 ? safe_normalize(-v) : vec3(0,0,1);
        vec3 lit=e.rgb+a.rgb*s[16].rgb;
        for(int i=0;i<8;i++) {
            int b=32+i*7;
            if(s[b+6].x==0.0) continue;
            vec4 lp=s[b];
            vec3 delta=lp.w==0.0 ? lp.xyz : lp.xyz/lp.w-v;
            float distance=length(delta);
            vec3 l=safe_normalize(delta);
            float attenuation=lp.w==0.0 ? 1.0 : 1.0/dot(s[b+5].xyz,vec3(1,distance,distance*distance));
            float spot=1.0;
            if(s[b+6].y==0.0) {
                float sd=max(dot(-l,safe_normalize(s[b+4].xyz)),0.0);
                spot=sd<s[b+4].w ? 0.0 : safe_pow(sd,s[b+5].w);
            }
            float diffuse=max(dot(n,l),0.0);
            float specular=diffuse>0.0 ? safe_pow(max(dot(n,safe_normalize(l+viewer)),0.0),s[21].x) : 0.0;
            lit+=attenuation*spot*(a.rgb*s[b+1].rgb+diffuse*d.rgb*s[b+2].rgb+specular*p.rgb*s[b+3].rgb);
        }
        primary=clamp(vec4(lit,d.a),0.0,1.0);
    }
}
