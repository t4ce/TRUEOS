#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LumenBackend {
    Cpu,
    CpuTrueos,
    Cuda,
}

impl LumenBackend {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::CpuTrueos => "cpu_trueos",
            Self::Cuda => "cuda",
        }
    }

    #[inline]
    pub const fn is_trueos(self) -> bool {
        matches!(self, Self::CpuTrueos)
    }
}

#[inline]
pub const fn trueos_target_compiled() -> bool {
    cfg!(any(target_os = "trueos", target_os = "zkvm", feature = "cpu-trueos"))
}

#[inline]
pub const fn default_backend() -> LumenBackend {
    if trueos_target_compiled() {
        LumenBackend::CpuTrueos
    } else if cfg!(feature = "cuda") {
        LumenBackend::Cuda
    } else {
        LumenBackend::Cpu
    }
}

#[inline]
pub const fn default_backend_name() -> &'static str {
    default_backend().as_str()
}

#[inline]
pub const fn trueos_parallel_contract() -> &'static str {
    "cooperative_chunked_ap_worker"
}
