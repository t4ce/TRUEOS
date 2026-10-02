//! Backend selection for Shell3 draw execution.

/// The execution path that will handle a show's draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    /// Execute draws on the CPU.
    #[default]
    Cpu,
    /// Execute draws through the render backend.
    Render,
    /// Execute draws through the copy backend.
    Copy,
}

/// Per-show draw backend selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Show {
    backend: Backend,
}

impl Show {
    pub const fn new(backend: Backend) -> Self {
        Self { backend }
    }

    pub const fn backend(&self) -> Backend {
        self.backend
    }

    pub fn set_backend(&mut self, backend: Backend) {
        self.backend = backend;
    }
}
