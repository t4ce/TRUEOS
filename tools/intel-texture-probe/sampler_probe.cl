// Offline Intel sampler-message probe. This is not a renderer and is never
// shipped in the kernel. It gives the bring-up tools one identical image read
// to compile for the physical ADL-S target and the hosted RPL-S oracle.
__constant sampler_t nearest_repeat =
    CLK_NORMALIZED_COORDS_TRUE |
    CLK_ADDRESS_REPEAT |
    CLK_FILTER_NEAREST;

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void sample_rgba8(
    read_only image2d_t image,
    __global float4 *out,
    float2 uv
) {
    out[0] = read_imagef(image, nearest_repeat, uv);
}
