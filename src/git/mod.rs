//! Git CLI adapter. All repository access goes through the `git` binary.

pub mod log;
pub mod ops;
pub mod read;
pub mod repo;
pub mod runner;

pub use runner::{GitError, Opts, Runner};
