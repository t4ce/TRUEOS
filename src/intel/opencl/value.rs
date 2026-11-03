//! Contract-checked construction of OpenCL cross-thread scalar/POD values.
//!
//! Buffer addresses, dispatch built-ins, and per-thread local IDs remain owned
//! by the backend. This writer only touches arguments declared as by-value in
//! the kernel contract, so higher layers do not need to know raw payload
//! offsets and cannot accidentally overwrite a surface pointer.

use super::artifact::{GpuKernelContract, KernelArgAccess, KernelArgKind};

const TRACKED_ARG_WORDS: usize = 4;
pub(crate) const MAX_TRACKED_VALUE_ARG_INDEX: u32 = (TRACKED_ARG_WORDS as u32 * 64) - 1;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KernelValueType {
    U32,
    I32,
    F32,
    U64,
    I64,
    F64,
    I32x4,
    U32x4,
    F32x4,
    Other,
}

impl KernelValueType {
    const fn label(self) -> &'static str {
        match self {
            Self::U32 => "uint",
            Self::I32 => "int",
            Self::F32 => "float",
            Self::U64 => "ulong",
            Self::I64 => "long",
            Self::F64 => "double",
            Self::I32x4 => "int4",
            Self::U32x4 => "uint4",
            Self::F32x4 => "float4",
            Self::Other => "other",
        }
    }

    fn from_contract_label(label: &str) -> Self {
        match label {
            "uint" => Self::U32,
            "int" => Self::I32,
            "float" => Self::F32,
            "ulong" => Self::U64,
            "long" => Self::I64,
            "double" => Self::F64,
            "int4" => Self::I32x4,
            "uint4" => Self::U32x4,
            "float4" => Self::F32x4,
            _ => Self::Other,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KernelValueError {
    CrossThreadBufferTooSmall {
        required: usize,
        actual: usize,
    },
    UnknownArgument {
        index: u32,
    },
    ArgumentIsNotByValue {
        index: u32,
        kind: KernelArgKind,
        access: KernelArgAccess,
    },
    MissingPayloadOffset {
        index: u32,
    },
    PayloadOffsetOverflow {
        index: u32,
    },
    MisalignedPayloadOffset {
        index: u32,
        offset_bytes: usize,
        align_bytes: usize,
    },
    ValueSizeMismatch {
        index: u32,
        expected: usize,
        actual: usize,
    },
    ValueTypeMismatch {
        index: u32,
        expected: KernelValueType,
        actual: KernelValueType,
    },
    PayloadOutOfBounds {
        index: u32,
        end_bytes: usize,
        cross_thread_bytes: usize,
    },
    ArgumentIndexTooLarge {
        index: u32,
        max: u32,
    },
    DuplicateValue {
        index: u32,
    },
    MissingValue {
        index: u32,
    },
}

/// Writes scalar/POD values into an existing cross-thread payload.
///
/// The caller may seed backend-owned built-ins and buffer addresses before or
/// after using this writer. `finish` requires every by-value contract argument
/// to have been supplied exactly once.
#[derive(Debug)]
pub(crate) struct KernelValueWriter<'contract, 'payload> {
    contract: &'contract GpuKernelContract<'contract>,
    payload: &'payload mut [u8],
    written: [u64; TRACKED_ARG_WORDS],
}

impl<'contract, 'payload> KernelValueWriter<'contract, 'payload> {
    pub(crate) fn new(
        contract: &'contract GpuKernelContract<'contract>,
        payload: &'payload mut [u8],
    ) -> Result<Self, KernelValueError> {
        let required = contract.cross_thread_bytes as usize;
        if payload.len() < required {
            return Err(KernelValueError::CrossThreadBufferTooSmall {
                required,
                actual: payload.len(),
            });
        }

        Ok(Self {
            contract,
            payload,
            written: [0; TRACKED_ARG_WORDS],
        })
    }

    pub(crate) fn set_u32(
        &mut self,
        index: u32,
        value: u32,
    ) -> Result<&mut Self, KernelValueError> {
        self.set_typed_bytes(index, KernelValueType::U32, &value.to_le_bytes())
    }

    pub(crate) fn set_i32(
        &mut self,
        index: u32,
        value: i32,
    ) -> Result<&mut Self, KernelValueError> {
        self.set_typed_bytes(index, KernelValueType::I32, &value.to_le_bytes())
    }

    pub(crate) fn set_f32(
        &mut self,
        index: u32,
        value: f32,
    ) -> Result<&mut Self, KernelValueError> {
        self.set_typed_bytes(index, KernelValueType::F32, &value.to_bits().to_le_bytes())
    }

    pub(crate) fn set_u64(
        &mut self,
        index: u32,
        value: u64,
    ) -> Result<&mut Self, KernelValueError> {
        self.set_typed_bytes(index, KernelValueType::U64, &value.to_le_bytes())
    }

    pub(crate) fn set_i64(
        &mut self,
        index: u32,
        value: i64,
    ) -> Result<&mut Self, KernelValueError> {
        self.set_typed_bytes(index, KernelValueType::I64, &value.to_le_bytes())
    }

    pub(crate) fn set_f64(
        &mut self,
        index: u32,
        value: f64,
    ) -> Result<&mut Self, KernelValueError> {
        self.set_typed_bytes(index, KernelValueType::F64, &value.to_bits().to_le_bytes())
    }

    pub(crate) fn set_i32x4(
        &mut self,
        index: u32,
        value: [i32; 4],
    ) -> Result<&mut Self, KernelValueError> {
        let mut bytes = [0u8; 16];
        for (lane, lane_value) in value.into_iter().enumerate() {
            let start = lane * 4;
            bytes[start..start + 4].copy_from_slice(&lane_value.to_le_bytes());
        }
        self.set_typed_bytes(index, KernelValueType::I32x4, &bytes)
    }

    pub(crate) fn set_u32x4(
        &mut self,
        index: u32,
        value: [u32; 4],
    ) -> Result<&mut Self, KernelValueError> {
        let mut bytes = [0u8; 16];
        for (lane, lane_value) in value.into_iter().enumerate() {
            let start = lane * 4;
            bytes[start..start + 4].copy_from_slice(&lane_value.to_le_bytes());
        }
        self.set_typed_bytes(index, KernelValueType::U32x4, &bytes)
    }

    pub(crate) fn set_f32x4(
        &mut self,
        index: u32,
        value: [f32; 4],
    ) -> Result<&mut Self, KernelValueError> {
        let mut bytes = [0u8; 16];
        for (lane, lane_value) in value.into_iter().enumerate() {
            let start = lane * 4;
            bytes[start..start + 4].copy_from_slice(&lane_value.to_bits().to_le_bytes());
        }
        self.set_typed_bytes(index, KernelValueType::F32x4, &bytes)
    }

    /// Writes an exact-size POD representation. Byte order and field layout
    /// are part of the declared POD contract and remain explicit at the call
    /// site; prefer the typed helpers for OpenCL scalar/vector types.
    pub(crate) fn set_pod_bytes(
        &mut self,
        index: u32,
        bytes: &[u8],
    ) -> Result<&mut Self, KernelValueError> {
        let (offset, size) = self.validate_value_slot(index, bytes.len())?;
        self.payload[offset..offset + size].copy_from_slice(bytes);
        self.mark_written(index);
        Ok(self)
    }

    pub(crate) fn finish(self) -> Result<&'payload mut [u8], KernelValueError> {
        for arg in self.contract.args {
            if is_by_value(arg.kind, arg.access) && !self.is_written(arg.index) {
                return Err(KernelValueError::MissingValue { index: arg.index });
            }
        }
        Ok(self.payload)
    }

    fn set_typed_bytes(
        &mut self,
        index: u32,
        value_type: KernelValueType,
        bytes: &[u8],
    ) -> Result<&mut Self, KernelValueError> {
        let arg = self
            .contract
            .args
            .iter()
            .find(|arg| arg.index == index)
            .ok_or(KernelValueError::UnknownArgument { index })?;
        if arg.type_name != value_type.label() {
            return Err(KernelValueError::ValueTypeMismatch {
                index,
                expected: KernelValueType::from_contract_label(arg.type_name),
                actual: value_type,
            });
        }
        self.set_pod_bytes(index, bytes)
    }

    fn validate_value_slot(
        &self,
        index: u32,
        actual_size: usize,
    ) -> Result<(usize, usize), KernelValueError> {
        let arg = self
            .contract
            .args
            .iter()
            .find(|arg| arg.index == index)
            .ok_or(KernelValueError::UnknownArgument { index })?;
        if !is_by_value(arg.kind, arg.access) {
            return Err(KernelValueError::ArgumentIsNotByValue {
                index,
                kind: arg.kind,
                access: arg.access,
            });
        }
        if index > MAX_TRACKED_VALUE_ARG_INDEX {
            return Err(KernelValueError::ArgumentIndexTooLarge {
                index,
                max: MAX_TRACKED_VALUE_ARG_INDEX,
            });
        }
        if self.is_written(index) {
            return Err(KernelValueError::DuplicateValue { index });
        }

        let expected_size = arg.size_bytes as usize;
        if expected_size != actual_size {
            return Err(KernelValueError::ValueSizeMismatch {
                index,
                expected: expected_size,
                actual: actual_size,
            });
        }
        let payload_dword = arg
            .payload_dword
            .ok_or(KernelValueError::MissingPayloadOffset { index })?;
        let offset = (payload_dword as usize)
            .checked_mul(core::mem::size_of::<u32>())
            .ok_or(KernelValueError::PayloadOffsetOverflow { index })?;
        let align = arg.align_bytes as usize;
        if align == 0 || offset % align != 0 {
            return Err(KernelValueError::MisalignedPayloadOffset {
                index,
                offset_bytes: offset,
                align_bytes: align,
            });
        }
        let end = offset
            .checked_add(expected_size)
            .ok_or(KernelValueError::PayloadOffsetOverflow { index })?;
        let cross_thread_bytes = self.contract.cross_thread_bytes as usize;
        if end > cross_thread_bytes || end > self.payload.len() {
            return Err(KernelValueError::PayloadOutOfBounds {
                index,
                end_bytes: end,
                cross_thread_bytes,
            });
        }
        Ok((offset, expected_size))
    }

    fn mark_written(&mut self, index: u32) {
        let word = index as usize / 64;
        let bit = index % 64;
        self.written[word] |= 1u64 << bit;
    }

    fn is_written(&self, index: u32) -> bool {
        if index > MAX_TRACKED_VALUE_ARG_INDEX {
            return false;
        }
        let word = index as usize / 64;
        let bit = index % 64;
        self.written[word] & (1u64 << bit) != 0
    }
}

const fn is_by_value(kind: KernelArgKind, access: KernelArgAccess) -> bool {
    matches!(kind, KernelArgKind::Scalar | KernelArgKind::Pod)
        && matches!(access, KernelArgAccess::ByValue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::opencl::artifact::{
        GpuArtifactProducer, KernelCallArg, KernelLaunchContract,
    };

    const ARGS: &[KernelCallArg<'_>] = &[
        KernelCallArg::buffer(0, "dst", "__global uint*", KernelArgAccess::WriteOnly, 0, 12),
        KernelCallArg::value(1, "phase", "float", 4, 4, 14),
        KernelCallArg::value_kind(2, "range", "int4", KernelArgKind::Pod, 16, 16, 16),
    ];
    const CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
        name: "chart_test",
        source_path: "chart_test.cl",
        producer: GpuArtifactProducer::IntelIgcOcloc,
        target: "adls",
        entry_text_offset_bytes: 0x40,
        cross_thread_bytes: 80,
        per_thread_bytes: 96,
        binding_count: 1,
        args: ARGS,
        descriptor_layouts: &[],
        launch: KernelLaunchContract::nd_range_2d(None),
        consumers: &[],
    };

    #[test]
    fn writes_typed_values_without_touching_backend_slots() {
        let mut payload = [0xa5; 80];
        let mut writer = KernelValueWriter::new(&CONTRACT, &mut payload).unwrap();
        writer.set_f32(1, 1.25).unwrap();
        writer.set_i32x4(2, [-1, 2, -3, 4]).unwrap();
        let payload = writer.finish().unwrap();

        assert_eq!(&payload[..56], &[0xa5; 56]);
        assert_eq!(&payload[56..60], &1.25f32.to_bits().to_le_bytes());
        assert_eq!(&payload[64..68], &(-1i32).to_le_bytes());
        assert_eq!(&payload[76..80], &4i32.to_le_bytes());
    }

    #[test]
    fn rejects_wrong_type_and_missing_value() {
        let mut payload = [0; 80];
        let mut writer = KernelValueWriter::new(&CONTRACT, &mut payload).unwrap();
        assert_eq!(
            writer.set_u32(1, 7).unwrap_err(),
            KernelValueError::ValueTypeMismatch {
                index: 1,
                expected: KernelValueType::F32,
                actual: KernelValueType::U32,
            }
        );
        writer.set_f32(1, 0.0).unwrap();
        assert_eq!(writer.finish().unwrap_err(), KernelValueError::MissingValue { index: 2 });
    }

    #[test]
    fn rejects_buffer_arguments_and_duplicate_values() {
        let mut payload = [0; 80];
        let mut writer = KernelValueWriter::new(&CONTRACT, &mut payload).unwrap();
        assert!(matches!(
            writer.set_pod_bytes(0, &[0; 8]),
            Err(KernelValueError::ArgumentIsNotByValue { index: 0, .. })
        ));
        writer.set_f32(1, 0.0).unwrap();
        assert_eq!(
            writer.set_f32(1, 1.0).unwrap_err(),
            KernelValueError::DuplicateValue { index: 1 }
        );
    }
}
