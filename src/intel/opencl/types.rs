#![allow(dead_code)]

use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

pub(crate) const CL_SUCCESS: i32 = 0;

pub(crate) type ClResult<T> = core::result::Result<T, ClError>;

#[repr(i32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClError {
    DeviceNotFound = -1,
    DeviceNotAvailable = -2,
    CompilerNotAvailable = -3,
    MemObjectAllocationFailure = -4,
    OutOfResources = -5,
    OutOfHostMemory = -6,
    ProfilingInfoNotAvailable = -7,
    MemCopyOverlap = -8,
    ImageFormatMismatch = -9,
    ImageFormatNotSupported = -10,
    BuildProgramFailure = -11,
    MapFailure = -12,
    MisalignedSubBufferOffset = -13,
    ExecStatusErrorForEventsInWaitList = -14,
    CompileProgramFailure = -15,
    LinkerNotAvailable = -16,
    LinkProgramFailure = -17,
    DevicePartitionFailed = -18,
    KernelArgInfoNotAvailable = -19,
    InvalidValue = -30,
    InvalidDeviceType = -31,
    InvalidPlatform = -32,
    InvalidDevice = -33,
    InvalidContext = -34,
    InvalidQueueProperties = -35,
    InvalidCommandQueue = -36,
    InvalidHostPtr = -37,
    InvalidMemObject = -38,
    InvalidImageFormatDescriptor = -39,
    InvalidImageSize = -40,
    InvalidSampler = -41,
    InvalidBinary = -42,
    InvalidBuildOptions = -43,
    InvalidProgram = -44,
    InvalidProgramExecutable = -45,
    InvalidKernelName = -46,
    InvalidKernelDefinition = -47,
    InvalidKernel = -48,
    InvalidArgIndex = -49,
    InvalidArgValue = -50,
    InvalidArgSize = -51,
    InvalidKernelArgs = -52,
    InvalidWorkDimension = -53,
    InvalidWorkGroupSize = -54,
    InvalidWorkItemSize = -55,
    InvalidGlobalOffset = -56,
    InvalidEventWaitList = -57,
    InvalidEvent = -58,
    InvalidOperation = -59,
    InvalidGlObject = -60,
    InvalidBufferSize = -61,
    InvalidMipLevel = -62,
    InvalidGlobalWorkSize = -63,
    InvalidProperty = -64,
    InvalidImageDescriptor = -65,
    InvalidCompilerOptions = -66,
    InvalidLinkerOptions = -67,
    InvalidDevicePartitionCount = -68,
}

impl ClError {
    pub(crate) const fn code(self) -> i32 {
        self as i32
    }

    pub(crate) const fn from_code(code: i32) -> Option<Self> {
        match code {
            -1 => Some(Self::DeviceNotFound),
            -2 => Some(Self::DeviceNotAvailable),
            -3 => Some(Self::CompilerNotAvailable),
            -4 => Some(Self::MemObjectAllocationFailure),
            -5 => Some(Self::OutOfResources),
            -6 => Some(Self::OutOfHostMemory),
            -7 => Some(Self::ProfilingInfoNotAvailable),
            -8 => Some(Self::MemCopyOverlap),
            -9 => Some(Self::ImageFormatMismatch),
            -10 => Some(Self::ImageFormatNotSupported),
            -11 => Some(Self::BuildProgramFailure),
            -12 => Some(Self::MapFailure),
            -13 => Some(Self::MisalignedSubBufferOffset),
            -14 => Some(Self::ExecStatusErrorForEventsInWaitList),
            -15 => Some(Self::CompileProgramFailure),
            -16 => Some(Self::LinkerNotAvailable),
            -17 => Some(Self::LinkProgramFailure),
            -18 => Some(Self::DevicePartitionFailed),
            -19 => Some(Self::KernelArgInfoNotAvailable),
            -30 => Some(Self::InvalidValue),
            -31 => Some(Self::InvalidDeviceType),
            -32 => Some(Self::InvalidPlatform),
            -33 => Some(Self::InvalidDevice),
            -34 => Some(Self::InvalidContext),
            -35 => Some(Self::InvalidQueueProperties),
            -36 => Some(Self::InvalidCommandQueue),
            -37 => Some(Self::InvalidHostPtr),
            -38 => Some(Self::InvalidMemObject),
            -39 => Some(Self::InvalidImageFormatDescriptor),
            -40 => Some(Self::InvalidImageSize),
            -41 => Some(Self::InvalidSampler),
            -42 => Some(Self::InvalidBinary),
            -43 => Some(Self::InvalidBuildOptions),
            -44 => Some(Self::InvalidProgram),
            -45 => Some(Self::InvalidProgramExecutable),
            -46 => Some(Self::InvalidKernelName),
            -47 => Some(Self::InvalidKernelDefinition),
            -48 => Some(Self::InvalidKernel),
            -49 => Some(Self::InvalidArgIndex),
            -50 => Some(Self::InvalidArgValue),
            -51 => Some(Self::InvalidArgSize),
            -52 => Some(Self::InvalidKernelArgs),
            -53 => Some(Self::InvalidWorkDimension),
            -54 => Some(Self::InvalidWorkGroupSize),
            -55 => Some(Self::InvalidWorkItemSize),
            -56 => Some(Self::InvalidGlobalOffset),
            -57 => Some(Self::InvalidEventWaitList),
            -58 => Some(Self::InvalidEvent),
            -59 => Some(Self::InvalidOperation),
            -60 => Some(Self::InvalidGlObject),
            -61 => Some(Self::InvalidBufferSize),
            -62 => Some(Self::InvalidMipLevel),
            -63 => Some(Self::InvalidGlobalWorkSize),
            -64 => Some(Self::InvalidProperty),
            -65 => Some(Self::InvalidImageDescriptor),
            -66 => Some(Self::InvalidCompilerOptions),
            -67 => Some(Self::InvalidLinkerOptions),
            -68 => Some(Self::InvalidDevicePartitionCount),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClStatus {
    Success,
    Error(ClError),
}

impl ClStatus {
    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::Success => CL_SUCCESS,
            Self::Error(error) => error.code(),
        }
    }

    pub(crate) const fn from_code(code: i32) -> Option<Self> {
        match code {
            CL_SUCCESS => Some(Self::Success),
            _ => match ClError::from_code(code) {
                Some(error) => Some(Self::Error(error)),
                None => None,
            },
        }
    }

    pub(crate) const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }

    pub(crate) const fn into_result(self) -> ClResult<()> {
        match self {
            Self::Success => Ok(()),
            Self::Error(error) => Err(error),
        }
    }
}

macro_rules! id_newtype {
    ($name:ident) => {
        #[repr(transparent)]
        #[derive(Copy, Clone, Default, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) const INVALID: Self = Self(0);

            pub(crate) const fn new(raw: u32) -> Option<Self> {
                if raw == 0 { None } else { Some(Self(raw)) }
            }

            pub(crate) const fn from_raw(raw: u32) -> Self {
                Self(raw)
            }

            pub(crate) const fn raw(self) -> u32 {
                self.0
            }

            pub(crate) const fn is_valid(self) -> bool {
                self.0 != 0
            }
        }
    };
}

id_newtype!(PlatformId);
id_newtype!(DeviceId);
id_newtype!(ContextId);
id_newtype!(QueueId);
id_newtype!(ProgramId);
id_newtype!(KernelId);
id_newtype!(MemId);
id_newtype!(EventId);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DeviceKind {
    Default,
    Cpu,
    Gpu,
    Accelerator,
    Custom,
    All,
}

impl DeviceKind {
    pub(crate) const fn cl_bits(self) -> u64 {
        match self {
            Self::Default => 1 << 0,
            Self::Cpu => 1 << 1,
            Self::Gpu => 1 << 2,
            Self::Accelerator => 1 << 3,
            Self::Custom => 1 << 4,
            Self::All => 0xFFFF_FFFF,
        }
    }
}

macro_rules! bitflags_newtype {
    ($name:ident, $bits:ty, $valid:expr, {$($flag:ident = $value:expr,)*}) => {
        #[repr(transparent)]
        #[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
        pub(crate) struct $name($bits);

        impl $name {
            pub(crate) const EMPTY: Self = Self(0);
            pub(crate) const VALID_BITS: $bits = $valid;
            $(pub(crate) const $flag: Self = Self($value);)*

            pub(crate) const fn from_bits(bits: $bits) -> Option<Self> {
                if bits & !Self::VALID_BITS == 0 {
                    Some(Self(bits))
                } else {
                    None
                }
            }

            pub(crate) const fn from_bits_truncate(bits: $bits) -> Self {
                Self(bits & Self::VALID_BITS)
            }

            pub(crate) const fn bits(self) -> $bits {
                self.0
            }

            pub(crate) const fn is_empty(self) -> bool {
                self.0 == 0
            }

            pub(crate) const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            pub(crate) const fn intersects(self, other: Self) -> bool {
                self.0 & other.0 != 0
            }

            pub(crate) fn insert(&mut self, other: Self) {
                self.0 |= other.0;
            }

            pub(crate) fn remove(&mut self, other: Self) {
                self.0 &= !other.0;
            }
        }

        impl BitOr for $name {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self::Output {
                Self(self.0 | rhs.0)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0;
            }
        }

        impl BitAnd for $name {
            type Output = Self;

            fn bitand(self, rhs: Self) -> Self::Output {
                Self(self.0 & rhs.0)
            }
        }

        impl BitAndAssign for $name {
            fn bitand_assign(&mut self, rhs: Self) {
                self.0 &= rhs.0;
            }
        }

        impl Not for $name {
            type Output = Self;

            fn not(self) -> Self::Output {
                Self(!self.0 & Self::VALID_BITS)
            }
        }
    };
}

bitflags_newtype!(AccessFlags, u32, 0x3, {
    READ = 1 << 0,
    WRITE = 1 << 1,
});

impl AccessFlags {
    pub(crate) const READ_ONLY: Self = Self::READ;
    pub(crate) const WRITE_ONLY: Self = Self::WRITE;
    pub(crate) const READ_WRITE: Self = Self(Self::READ.bits() | Self::WRITE.bits());

    pub(crate) const fn can_read(self) -> bool {
        self.contains(Self::READ)
    }

    pub(crate) const fn can_write(self) -> bool {
        self.contains(Self::WRITE)
    }
}

bitflags_newtype!(MemFlags, u64, 0x0000_0000_0000_13BF, {
    READ_WRITE = 1 << 0,
    WRITE_ONLY = 1 << 1,
    READ_ONLY = 1 << 2,
    USE_HOST_PTR = 1 << 3,
    ALLOC_HOST_PTR = 1 << 4,
    COPY_HOST_PTR = 1 << 5,
    HOST_WRITE_ONLY = 1 << 7,
    HOST_READ_ONLY = 1 << 8,
    HOST_NO_ACCESS = 1 << 9,
    KERNEL_READ_AND_WRITE = 1 << 12,
});

impl MemFlags {
    pub(crate) const fn device_access(self) -> AccessFlags {
        if self.contains(Self::READ_ONLY) {
            AccessFlags::READ
        } else if self.contains(Self::WRITE_ONLY) {
            AccessFlags::WRITE
        } else {
            AccessFlags::READ_WRITE
        }
    }

    pub(crate) const fn host_access(self) -> AccessFlags {
        if self.contains(Self::HOST_NO_ACCESS) {
            AccessFlags::EMPTY
        } else if self.contains(Self::HOST_READ_ONLY) {
            AccessFlags::READ
        } else if self.contains(Self::HOST_WRITE_ONLY) {
            AccessFlags::WRITE
        } else {
            AccessFlags::READ_WRITE
        }
    }
}

bitflags_newtype!(QueueProperties, u64, 0xF, {
    OUT_OF_ORDER_EXEC_MODE_ENABLE = 1 << 0,
    PROFILING_ENABLE = 1 << 1,
    ON_DEVICE = 1 << 2,
    ON_DEVICE_DEFAULT = 1 << 3,
});

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct NdRange {
    work_dim: u8,
    global_offset: [usize; 3],
    global_size: [usize; 3],
    local_size: Option<[usize; 3]>,
}

impl NdRange {
    pub(crate) fn new(
        work_dim: u8,
        global_offset: [usize; 3],
        global_size: [usize; 3],
        local_size: Option<[usize; 3]>,
    ) -> ClResult<Self> {
        let range = Self {
            work_dim,
            global_offset,
            global_size,
            local_size,
        };
        range.validate()?;
        Ok(range)
    }

    pub(crate) const fn new_1d(global_size: usize) -> Self {
        Self {
            work_dim: 1,
            global_offset: [0, 0, 0],
            global_size: [global_size, 0, 0],
            local_size: None,
        }
    }

    pub(crate) const fn new_2d(global_width: usize, global_height: usize) -> Self {
        Self {
            work_dim: 2,
            global_offset: [0, 0, 0],
            global_size: [global_width, global_height, 0],
            local_size: None,
        }
    }

    pub(crate) const fn new_3d(
        global_width: usize,
        global_height: usize,
        global_depth: usize,
    ) -> Self {
        Self {
            work_dim: 3,
            global_offset: [0, 0, 0],
            global_size: [global_width, global_height, global_depth],
            local_size: None,
        }
    }

    pub(crate) const fn with_global_offset(mut self, global_offset: [usize; 3]) -> Self {
        self.global_offset = global_offset;
        self
    }

    pub(crate) const fn with_local_size(mut self, local_size: [usize; 3]) -> Self {
        self.local_size = Some(local_size);
        self
    }

    pub(crate) const fn work_dim(self) -> u8 {
        self.work_dim
    }

    pub(crate) const fn global_offset_array(self) -> [usize; 3] {
        self.global_offset
    }

    pub(crate) const fn global_size_array(self) -> [usize; 3] {
        self.global_size
    }

    pub(crate) const fn local_size_array(self) -> Option<[usize; 3]> {
        self.local_size
    }

    pub(crate) fn global_offset(&self) -> &[usize] {
        &self.global_offset[..self.work_dim as usize]
    }

    pub(crate) fn global_size(&self) -> &[usize] {
        &self.global_size[..self.work_dim as usize]
    }

    pub(crate) fn local_size(&self) -> Option<&[usize]> {
        self.local_size
            .as_ref()
            .map(|local_size| &local_size[..self.work_dim as usize])
    }

    pub(crate) fn validate(&self) -> ClResult<()> {
        let work_dim = self.work_dim as usize;
        if work_dim == 0 || work_dim > 3 {
            return Err(ClError::InvalidWorkDimension);
        }

        for axis in 0..3 {
            if axis < work_dim {
                if self.global_size[axis] == 0 {
                    return Err(ClError::InvalidGlobalWorkSize);
                }
                if let Some(local_size) = self.local_size {
                    if local_size[axis] == 0 || local_size[axis] > self.global_size[axis] {
                        return Err(ClError::InvalidWorkGroupSize);
                    }
                    if self.global_size[axis] % local_size[axis] != 0 {
                        return Err(ClError::InvalidWorkGroupSize);
                    }
                }
            } else {
                if self.global_offset[axis] != 0 || self.global_size[axis] != 0 {
                    return Err(ClError::InvalidValue);
                }
                if let Some(local_size) = self.local_size {
                    if local_size[axis] != 0 {
                        return Err(ClError::InvalidValue);
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn global_work_items(&self) -> usize {
        self.global_size()
            .iter()
            .copied()
            .fold(1usize, usize::saturating_mul)
    }

    pub(crate) fn local_work_items(&self) -> Option<usize> {
        self.local_size().map(|local_size| {
            local_size
                .iter()
                .copied()
                .fold(1usize, usize::saturating_mul)
        })
    }
}
