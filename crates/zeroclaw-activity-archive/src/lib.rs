//! ZeroClaw Activity Archive
//!
//! A comprehensive desktop activity tracking and archival system.
//! Collects multiple activity streams, normalizes them, infers sessions,
//! generates summaries, and syncs to Notion.

pub mod schema;
pub mod collector;
pub mod collectors;
pub mod normalizer;
pub mod sessionizer;
pub mod summarizer;
pub mod notion_sync;
pub mod privacy;
pub mod runtime;

pub use schema::*;
pub use collector::*;
pub use normalizer::*;
pub use sessionizer::*;
pub use summarizer::*;
pub use notion_sync::*;
pub use privacy::*;
pub use runtime::*;
