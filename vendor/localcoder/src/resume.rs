#[derive(Debug, Clone)]
pub enum ResumeTarget {
    New,
    ContinueLatest,
    ResumeId(String),
}
