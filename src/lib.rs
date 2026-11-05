//! `beads` - Agent-first issue tracker library
//!
//! This crate provides the core functionality for the `br` CLI tool,
//! a Rust port of the classic beads issue tracker.
//!
//! # Architecture
//!
//! The crate is organized into the following modules:
//!
//! - [`cli`] - Command-line interface using clap
//! - [`model`] - Data types (Issue, Dependency, Comment, Event)
//! - [`storage`] - `SQLite` database layer
//! - [`sync`] - JSONL import/export operations
//! - [`config`] - Configuration management
//! - [`coordination`] - Pure swarm coordination evidence contracts
//! - [`error`] - Error types and handling
//! - [`format`] - Output formatting (text, JSON)
//! - [`util`] - Utility functions (hashing, time, paths)

#![deny(unsafe_code)]
#![allow(clippy::module_name_repetitions)]

pub(crate) mod agent_registry;
pub mod analysis;
pub mod cli;
pub mod close_policy;
pub mod config;
pub mod coordination;
pub mod elm;
pub mod error;
pub mod format;
#[allow(
    clippy::must_use_candidate,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::use_self,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::manual_let_else,
    clippy::clone_on_copy,
    clippy::cloned_instead_of_copied,
    clippy::semicolon_if_nothing_returned,
    clippy::unreadable_literal,
    clippy::items_after_statements,
    clippy::missing_panics_doc,
    clippy::suboptimal_flops,
    clippy::collapsible_if,
    clippy::new_without_default
)]
pub mod graph;
pub mod health;
// governed-by: ADR-0005
pub mod hold;
pub mod inheritance;
pub mod logging;
pub mod model;
pub mod output;
pub mod shutdown;
pub mod storage;
pub mod sync;
pub mod tui;
pub mod util;
pub mod validation;
pub mod verify;

pub use error::{BeadsError, ErrorCode, Result, StructuredError};

/// Run the CLI application.
///
/// This is the main entry point called from `main()`.
///
/// # Errors
///
/// Returns an error if command execution fails.
#[allow(clippy::missing_const_for_fn)]
pub fn run() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_returns_ok() {
        assert!(run().is_ok());
    }

    #[test]
    fn structured_error_exit_code_matches_issue_errors() {
        let err = BeadsError::IssueNotFound {
            id: "bd-xyz".to_string(),
        };
        let structured = StructuredError::from_error(&err);
        assert_eq!(structured.code, ErrorCode::IssueNotFound);
        assert_eq!(structured.code.exit_code(), 3);
    }

    #[cfg(feature = "self_update")]
    #[test]
    fn upgrade_module_is_available_when_feature_enabled() {
        let _ = crate::cli::commands::upgrade::execute;
    }
}
