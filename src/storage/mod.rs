//! `SQLite` storage layer for `beads`.
//!
//! This module provides the persistence layer using `SQLite` with:
//! - WAL mode for concurrent reads
//! - Transaction discipline for atomic writes
//! - Dirty tracking for JSONL export
//! - Blocked cache for ready/blocked queries
//!
//! # Submodules
//!
//! - [`db`] - rusqlite engine boundary (ADR-0002): connection, rows, values,
//!   and the caller-facing [`db::DbError`] taxonomy
//! - [`events`] - Audit event storage (insertion, retrieval)
//! - [`schema`] - Database schema definitions
//! - [`sqlite`] - Main `SQLite` storage implementation

pub mod db;
pub mod events;
pub mod schema;
pub mod sqlite;

pub use db::{Connection, DbError, PreparedStatement, Row, SqliteValue};
pub(crate) use sqlite::BulkDependencyInsert;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use sqlite::ChangelogIssueRow;

pub use sqlite::{
    CloseMetadataRow, CloseMetadataUpdate, EventAttribution, IssueUpdate, ListFilters,
    ReadyFilters, ReadySortPolicy, SqliteStorage, StatsIssueRow,
};
