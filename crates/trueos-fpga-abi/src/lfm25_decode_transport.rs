//! Fixed BAR0 envelope for the ahead-of-time LFM2.5 decode circuits.
//!
//! This is a register contract, not an instruction stream. Each request names one
//! circuit from [`DecodeOpKind`] and routes FPGA-resident tensors between fixed storage
//! slots. The matching firmware must publish the exact v1 magic and capability word
//! before the host may ring the doorbell.

use super::lfm25;
use super::lfm25_decode::{DecodeCapabilities, DecodeOpKind};

/// Separately versioned decode transport magic: bytes `TGD1` in BAR byte order.
pub const CAPABILITY_MAGIC: u32 = 0x3144_4754;
/// The v1 firmware must implement the complete fixed decode operation set.
pub const REQUIRED_CAPABILITY_BITS: u32 = DecodeCapabilities::ALL.bits() as u32;
/// Doorbell value for one already-published fixed decode request.
pub const DOORBELL_MAGIC: u32 = 0x4F43_4544; // "DECO"

/// The decode register plane occupies every currently free dword before the work package.
pub const BAR0_CAPABILITY_MAGIC_OFFSET: usize = 0x0DC;
pub const BAR0_CAPABILITY_BITS_OFFSET: usize = 0x0E0;
pub const BAR0_COMMAND_OFFSET: usize = 0x0E4;
pub const BAR0_POSITION_OFFSET: usize = 0x0E8;
pub const BAR0_SESSION_EPOCH_OFFSET: usize = 0x0EC;
pub const BAR0_DOORBELL_OFFSET: usize = 0x0F0;
pub const BAR0_STATE_OFFSET: usize = 0x0F4;
pub const BAR0_RESULT0_OFFSET: usize = 0x0F8;
pub const BAR0_RESULT1_OFFSET: usize = 0x0FC;
/// Argmax alone reuses the row lane's existing signed 64-bit result pair. The transports
/// are mutually exclusive, so this preserves the score without widening the free gap.
pub const BAR0_ARGMAX_SCORE_LO_OFFSET: usize = super::BAR0_LFM25_STREAM_RESULT_LO_OFFSET;
pub const BAR0_ARGMAX_SCORE_HI_OFFSET: usize = super::BAR0_LFM25_STREAM_RESULT_HI_OFFSET;

pub const NO_LAYER: u8 = u8::MAX;
pub const NO_RESIDENT_SLOT: u8 = u8::MAX;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum State {
    Idle = 0,
    Busy = 1,
    Complete = 2,
    Failed = 3,
}

impl State {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Busy),
            2 => Some(Self::Complete),
            3 => Some(Self::Failed),
            _ => None,
        }
    }
}

/// One fixed decode invocation. Slots are circuit-owned storage identities, never BAR
/// addresses or host pointers. Slot 255 is reserved as the wire encoding for `None`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub operation: DecodeOpKind,
    pub layer: Option<u8>,
    pub position: u32,
    pub input_slot: Option<u8>,
    pub residual_slot: Option<u8>,
    /// A nonzero resident-state epoch. Position-0 `TokenEmbedding` installs and resets
    /// this epoch in hardware; every later operation must match the installed value.
    pub session_epoch: u32,
}

impl Command {
    pub const fn control_word(self) -> u32 {
        (self.operation as u32)
            | ((encode_optional(self.layer, NO_LAYER) as u32) << 8)
            | ((encode_optional(self.input_slot, NO_RESIDENT_SLOT) as u32) << 16)
            | ((encode_optional(self.residual_slot, NO_RESIDENT_SLOT) as u32) << 24)
    }

    pub fn validate(self) -> Result<(), CodecError> {
        if self.session_epoch == 0 {
            return Err(CodecError::InvalidSessionEpoch);
        }
        if self
            .layer
            .is_some_and(|layer| layer as usize >= lfm25::MODEL_LAYER_COUNT)
        {
            return Err(CodecError::InvalidLayer);
        }
        if self.input_slot == Some(NO_RESIDENT_SLOT) || self.residual_slot == Some(NO_RESIDENT_SLOT)
        {
            return Err(CodecError::InvalidResidentSlot);
        }

        let shape_valid = match self.operation {
            DecodeOpKind::TokenEmbedding => {
                self.layer.is_none() && self.input_slot.is_none() && self.residual_slot.is_none()
            }
            DecodeOpKind::OperatorRmsNorm
            | DecodeOpKind::ShortConv
            | DecodeOpKind::Attention
            | DecodeOpKind::FfnRmsNorm
            | DecodeOpKind::Ffn => {
                self.layer.is_some() && self.input_slot.is_some() && self.residual_slot.is_none()
            }
            DecodeOpKind::OperatorResidual | DecodeOpKind::FfnResidual => {
                self.layer.is_some() && self.input_slot.is_some() && self.residual_slot.is_some()
            }
            DecodeOpKind::FinalRmsNorm | DecodeOpKind::TiedLmHeadArgmax => {
                self.layer.is_none() && self.input_slot.is_some() && self.residual_slot.is_none()
            }
        };
        if !shape_valid {
            return Err(CodecError::InvalidCommandShape);
        }
        Ok(())
    }

    pub fn from_registers(
        control: u32,
        position: u32,
        session_epoch: u32,
    ) -> Result<Self, CodecError> {
        let operation = decode_operation(control as u8).ok_or(CodecError::InvalidOperation)?;
        let command = Self {
            operation,
            layer: decode_optional((control >> 8) as u8, NO_LAYER),
            input_slot: decode_optional((control >> 16) as u8, NO_RESIDENT_SLOT),
            residual_slot: decode_optional((control >> 24) as u8, NO_RESIDENT_SLOT),
            position,
            session_epoch,
        };
        command.validate()?;
        Ok(command)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Completion {
    Resident {
        storage_slot: u8,
        position: u32,
    },
    Argmax {
        token: u32,
        score_q30: i64,
        rows: u32,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    InvalidCapability,
    InvalidOperation,
    InvalidLayer,
    InvalidResidentSlot,
    InvalidSessionEpoch,
    InvalidCommandShape,
    InvalidState,
    InvalidCompletion,
    Device(i32),
}

/// v1 is deliberately exact. New bits require a new capability magic so an older host
/// cannot accidentally treat a widened command contract as compatible.
pub const fn capability_is_exact(magic: u32, bits: u32) -> bool {
    magic == CAPABILITY_MAGIC && bits == REQUIRED_CAPABILITY_BITS
}

pub fn decode_completion(
    command: Command,
    state: State,
    result0: u32,
    result1: u32,
    argmax_score_q30: i64,
) -> Result<Completion, CodecError> {
    command.validate()?;
    match state {
        State::Failed => Err(CodecError::Device(result0 as i32)),
        State::Complete if command.operation == DecodeOpKind::TiedLmHeadArgmax => {
            if result0 >= lfm25::MODEL_VOCABULARY_SIZE || result1 != lfm25::MODEL_VOCABULARY_SIZE {
                return Err(CodecError::InvalidCompletion);
            }
            Ok(Completion::Argmax {
                token: result0,
                score_q30: argmax_score_q30,
                rows: result1,
            })
        }
        State::Complete => {
            if result0 & !0xFF != 0 || result0 as u8 == NO_RESIDENT_SLOT {
                return Err(CodecError::InvalidCompletion);
            }
            if result1 != command.position {
                return Err(CodecError::InvalidCompletion);
            }
            Ok(Completion::Resident {
                storage_slot: result0 as u8,
                position: result1,
            })
        }
        State::Idle | State::Busy => Err(CodecError::InvalidState),
    }
}

/// Helper for firmware-model tests and generators. RTL implements this fixed bit layout
/// directly; TRUEOS uses only [`decode_completion`].
pub fn encode_completion(completion: Completion) -> Result<(u32, u32, i64), CodecError> {
    match completion {
        Completion::Resident {
            storage_slot,
            position,
        } => {
            if storage_slot == NO_RESIDENT_SLOT {
                return Err(CodecError::InvalidResidentSlot);
            }
            Ok((storage_slot as u32, position, 0))
        }
        Completion::Argmax {
            token,
            score_q30,
            rows,
        } => {
            if token >= lfm25::MODEL_VOCABULARY_SIZE || rows != lfm25::MODEL_VOCABULARY_SIZE {
                return Err(CodecError::InvalidCompletion);
            }
            Ok((token, rows, score_q30))
        }
    }
}

const fn encode_optional(value: Option<u8>, none: u8) -> u8 {
    match value {
        Some(value) => value,
        None => none,
    }
}

const fn decode_optional(value: u8, none: u8) -> Option<u8> {
    if value == none { None } else { Some(value) }
}

const fn decode_operation(raw: u8) -> Option<DecodeOpKind> {
    match raw {
        0 => Some(DecodeOpKind::TokenEmbedding),
        1 => Some(DecodeOpKind::OperatorRmsNorm),
        2 => Some(DecodeOpKind::ShortConv),
        3 => Some(DecodeOpKind::Attention),
        4 => Some(DecodeOpKind::OperatorResidual),
        5 => Some(DecodeOpKind::FfnRmsNorm),
        6 => Some(DecodeOpKind::Ffn),
        7 => Some(DecodeOpKind::FfnResidual),
        8 => Some(DecodeOpKind::FinalRmsNorm),
        9 => Some(DecodeOpKind::TiedLmHeadArgmax),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rmsnorm_command() -> Command {
        Command {
            operation: DecodeOpKind::OperatorRmsNorm,
            layer: Some(7),
            position: 41,
            input_slot: Some(3),
            residual_slot: None,
            session_epoch: 19,
        }
    }

    #[test]
    fn register_plane_fills_only_the_free_bar0_gap() {
        assert_eq!(BAR0_CAPABILITY_MAGIC_OFFSET, 0x0DC);
        assert_eq!(BAR0_RESULT1_OFFSET + 4, super::super::BAR0_WORK_PACKAGE_OFFSET);
        assert_eq!(BAR0_CAPABILITY_BITS_OFFSET - BAR0_CAPABILITY_MAGIC_OFFSET, 4);
        assert_eq!(BAR0_RESULT1_OFFSET - BAR0_CAPABILITY_MAGIC_OFFSET, 8 * 4);
    }

    #[test]
    fn v1_capability_is_fail_closed_and_exact() {
        assert!(capability_is_exact(CAPABILITY_MAGIC, REQUIRED_CAPABILITY_BITS));
        assert!(!capability_is_exact(0, REQUIRED_CAPABILITY_BITS));
        assert!(!capability_is_exact(CAPABILITY_MAGIC, REQUIRED_CAPABILITY_BITS ^ 1));
        assert!(!capability_is_exact(CAPABILITY_MAGIC, REQUIRED_CAPABILITY_BITS | (1 << 31)));
    }

    #[test]
    fn command_registers_round_trip_and_validate_shape() {
        let command = rmsnorm_command();
        let decoded = Command::from_registers(
            command.control_word(),
            command.position,
            command.session_epoch,
        )
        .unwrap();
        assert_eq!(decoded, command);

        let mut malformed = command;
        malformed.residual_slot = Some(4);
        assert_eq!(malformed.validate(), Err(CodecError::InvalidCommandShape));
        malformed = command;
        malformed.session_epoch = 0;
        assert_eq!(malformed.validate(), Err(CodecError::InvalidSessionEpoch));
    }

    #[test]
    fn resident_completion_is_bound_to_position() {
        let command = rmsnorm_command();
        let words = encode_completion(Completion::Resident {
            storage_slot: 9,
            position: command.position,
        })
        .unwrap();
        assert_eq!(
            decode_completion(command, State::Complete, words.0, words.1, words.2),
            Ok(Completion::Resident {
                storage_slot: 9,
                position: command.position,
            })
        );
        assert_eq!(
            decode_completion(command, State::Complete, words.0, words.1 + 1, words.2),
            Err(CodecError::InvalidCompletion)
        );
    }

    #[test]
    fn argmax_preserves_the_full_signed_q30_score() {
        for score in [i64::MIN, -9_429_888, 0, 13_098_259, i64::MAX] {
            let completion = Completion::Argmax {
                token: 65_535,
                score_q30: score,
                rows: lfm25::MODEL_VOCABULARY_SIZE,
            };
            let words = encode_completion(completion).unwrap();
            let command = Command {
                operation: DecodeOpKind::TiedLmHeadArgmax,
                layer: None,
                position: 2,
                input_slot: Some(4),
                residual_slot: None,
                session_epoch: 1,
            };
            assert_eq!(
                decode_completion(command, State::Complete, words.0, words.1, words.2),
                Ok(completion)
            );
        }
    }
}
