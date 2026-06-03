// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - No arguments.
// - No memory writes.
// - Exists only to produce a minimal compiled EU kernel that reaches EOT.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void empty_eot(void)
{
}

