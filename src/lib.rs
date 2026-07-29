//! Shared distribution runtime for Rust command-line applications.

mod build_support;
mod error;
mod secure_store;
mod update;

pub use build_support::{VersionFormat, cargo_version, emit_version_file, validate_version};
pub use error::{Error, Result};
pub use secure_store::{LockGuard, LockedJsonStore, SecureDir};
pub use update::{ReleaseSpec, UpdateSummary, compare_versions, update_current};
