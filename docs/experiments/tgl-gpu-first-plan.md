# Tiger Lake GPU-first probe

This follow-up intentionally prioritizes one observable result: Spirit must be allowed to reach the physical iGPU and present a frame before USB, NIC, shell, or diagnostic services matter.

The existing 0x9A49 -> 0x4680/rev0c experimental PCI alias already removes the generated ADL-S artifact device/revision seal. This follow-up removes later software readiness vetoes from Spirit's path on the same physical Tiger Lake probe only.
