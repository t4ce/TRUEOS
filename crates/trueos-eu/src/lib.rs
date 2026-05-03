#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod gfx12;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EuIsa {
    Gfx12,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EuArtifactKind {
    ThreadSpawnerEot,
    ThreadSpawnerEotSend1,
    GatewayEot,
    Hdc1BtiStoreThenThreadSpawnerEot,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EuArtifact {
    pub name: &'static str,
    pub isa: EuIsa,
    pub kind: EuArtifactKind,
    pub words: &'static [u32],
    pub expects_store: bool,
}

impl EuArtifact {
    pub const fn bytes_len(self) -> usize {
        self.words.len() * core::mem::size_of::<u32>()
    }

    pub const fn is_empty(self) -> bool {
        self.words.is_empty()
    }
}
