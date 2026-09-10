// Rust
pub mod api;
/// Shared rendering, not part of the crate's public surface: the shapes here
/// are an adapter concern.
pub(crate) mod describe;
pub mod link;
pub mod mcp;
pub mod redaction;
pub mod telegram;
