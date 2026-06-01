use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResponseSignature {
    pub status: u16,
    pub size: usize,
    pub words: usize,
    pub lines: usize,
    pub elapsed_ms: u128,
    pub location: Option<String>,
    pub title: Option<String>,
    pub body_hash: u64,
}
