/// endpoint URL, or import. This is not a full AST parser — it is a fast,
/// zero-dependency callsite finder designed to produce actionable results
/// without requiring language-specific parser crates.
mod locator;
mod types;

pub use locator::{locate_callsites, locate_callsites_in_source};
pub use types::{Callsite, CallsiteKind, ScanConfig, ScanResult};
