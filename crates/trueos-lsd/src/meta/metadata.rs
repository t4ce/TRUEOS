use v::vfs::{FsNodeKind, FsStat};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    kind: FileKind,
    len: u64,
}

impl Metadata {
    pub fn from_stat(stat: FsStat) -> Self {
        let kind = match stat.kind {
            FsNodeKind::File => FileKind::File,
            FsNodeKind::Directory => FileKind::Directory,
        };
        Self {
            kind,
            len: stat.len,
        }
    }

    pub fn kind(&self) -> FileKind {
        self.kind
    }

    pub fn file_type(&self) -> FileKind {
        self.kind
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, FileKind::File)
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, FileKind::Directory)
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self.kind, FileKind::Symlink)
    }

    pub fn len(&self) -> u64 {
        self.len
    }
}
