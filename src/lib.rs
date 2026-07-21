//! Shared distribution runtime for Rust command-line applications.

mod build_support;
mod error;
mod update;

pub use build_support::{VersionFormat, cargo_version, emit_version_file, validate_version};
pub use error::{Error, Result};
pub use update::{ReleaseSpec, UpdateSummary, compare_versions, update_current};
