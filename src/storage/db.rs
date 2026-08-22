//! governed-by: ADR-0002
//!
//! Synchronous database layer over rusqlite (ADR-0002 W2).
//!
//! This module is the engine boundary for br's storage: every SQL statement
//! in the crate runs through [`Connection`] or [`PreparedStatement`]. The
//! adapter preserves the call shape storage code was written against
//! (buffered owned rows, strict `query_row`, tolerant `execute`) so the
//! behavior above this line is unchanged; what changed underneath is the
//! engine itself:
//!
//! * WAL mode and `PRAGMA user_version` stamping are native SQLite features,
//!   driven by `storage::schema` exactly as before.
//! * The deleted engine-specific caller-side retry window is replaced
//!   by a real SQLite busy handler ([`DEFAULT_BUSY_TIMEOUT`]); an explicit
//!   `PRAGMA busy_timeout=N` issued by callers overrides it.
//! * There is no async runtime: the thread-local driver bridge that used to
//!   live in `franken_sync.rs` is deleted outright.
//!
//! [`DbError`] mirrors the error taxonomy callers classify on (JSONL
//! recovery triage, doctor unavailability buckets, dedup collision checks)
//! with the same field shapes and Display wording the old engine produced;
//! [`Connection::db_error`] maps raw rusqlite failures into it in one place.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::ffi;
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Error as RError, OpenFlags as ROpenFlags, Statement, ToSql, params_from_iter};

/// Engine busy-handler window installed on every open (ADR-0002 §3).
///
/// Replaces the deleted BusyRecovery bounded-retry budget; callers that need
/// different contention behavior issue their own `PRAGMA busy_timeout`
/// afterwards, which wins.
const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Storage-layer database error.
///
/// Variant set is exactly what callers outside this module match on; the
/// Display strings reproduce the engine wording those classifiers were
/// written against (`doctor`'s "unable to open database file" bucket, the
/// JSONL recovery triage, dedup collision detection).
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Database file is locked by another process.
    #[error("database is locked: '{path}'")]
    DatabaseLocked { path: PathBuf },

    /// Database file is corrupt.
    #[error("database disk image is malformed: {detail}")]
    DatabaseCorrupt { detail: String },

    /// Database file is not a valid SQLite database.
    #[error("file is not a database: '{path}'")]
    NotADatabase { path: PathBuf },

    /// Database schema changed under a prepared statement.
    #[error("database schema has changed")]
    SchemaChanged,

    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Short read (fewer bytes than expected).
    ///
    /// The engine no longer reports expected/actual byte counts; the zeros
    /// only appear when this variant is produced by classification rather
    /// than constructed by a test or classifier.
    #[error("short read: expected {expected} bytes, got {actual}")]
    ShortRead { expected: usize, actual: usize },

    /// Query executed successfully but produced no rows.
    #[error("query returned no rows")]
    QueryReturnedNoRows,

    /// Query executed successfully but produced more than one row.
    #[error("query returned more than one row")]
    QueryReturnedMultipleRows,

    /// Table already exists.
    #[error("table {name} already exists")]
    TableExists { name: String },

    /// Index already exists.
    #[error("index {name} already exists")]
    IndexExists { name: String },

    /// UNIQUE constraint violation.
    #[error("UNIQUE constraint failed: {columns}")]
    UniqueViolation { columns: String },

    /// PRIMARY KEY constraint violation.
    #[error("PRIMARY KEY constraint failed")]
    PrimaryKeyViolation,

    /// Database is busy.
    #[error("database is busy")]
    Busy,

    /// WAL file is corrupt.
    #[error("WAL file is corrupt: {detail}")]
    WalCorrupt { detail: String },

    /// File locking failed.
    #[error("file locking failed: {detail}")]
    LockFailed { detail: String },

    /// Cannot open file.
    #[error("unable to open database file: '{path}'")]
    CannotOpen { path: PathBuf },

    /// Internal logic error.
    #[error("internal error: {0}")]
    Internal(String),

    /// An engine condition this layer does not classify further.
    #[error("{0}")]
    Engine(String),
}

impl DbError {
    /// True when the error is contention-shaped and a bounded retry may
    /// succeed (ADR-0002 §3: under real SQLite this is the busy/locked
    /// family; the MVCC-specific shapes of the old engine cannot occur).
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Busy | Self::DatabaseLocked { .. } | Self::LockFailed { .. }
        )
    }

    /// Map a raw rusqlite failure into the caller-facing taxonomy.
    ///
    /// `path` supplies the connection path for open/lock-shaped variants;
    /// classification happens once, here, so every caller sees the same
    /// shapes regardless of which adapter method failed.
    pub(crate) fn classify(err: RError, path: Option<&str>) -> Self {
        let conn_path = || PathBuf::from(path.unwrap_or_default());
        match err {
            RError::QueryReturnedNoRows => Self::QueryReturnedNoRows,
            RError::QueryReturnedMoreThanOneRow => Self::QueryReturnedMultipleRows,
            RError::InvalidPath(_) => Self::CannotOpen { path: conn_path() },
            RError::ExecuteReturnedResults => {
                Self::Internal("execute returned results".to_string())
            }
            RError::SqliteFailure(failure, message) => {
                let detail = message.unwrap_or_else(|| failure.to_string());
                let extended = failure.extended_code;
                if extended == ffi::SQLITE_CONSTRAINT_PRIMARYKEY {
                    return Self::PrimaryKeyViolation;
                }
                if extended == ffi::SQLITE_CONSTRAINT_UNIQUE {
                    let columns = detail
                        .strip_prefix("UNIQUE constraint failed: ")
                        .unwrap_or(&detail)
                        .to_string();
                    return Self::UniqueViolation { columns };
                }
                if extended == ffi::SQLITE_IOERR_SHORT_READ {
                    return Self::ShortRead {
                        expected: 0,
                        actual: 0,
                    };
                }
                match extended & 0xFF {
                    code if code == ffi::SQLITE_BUSY => Self::Busy,
                    code if code == ffi::SQLITE_LOCKED => {
                        Self::DatabaseLocked { path: conn_path() }
                    }
                    code if code == ffi::SQLITE_CANTOPEN => Self::CannotOpen { path: conn_path() },
                    code if code == ffi::SQLITE_CORRUPT => Self::DatabaseCorrupt { detail },
                    code if code == ffi::SQLITE_NOTADB => Self::NotADatabase { path: conn_path() },
                    code if code == ffi::SQLITE_SCHEMA => Self::SchemaChanged,
                    _ => Self::Engine(detail),
                }
            }
            other => Self::Engine(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Values and rows
// ---------------------------------------------------------------------------

/// A single SQLite value owned by a [`Row`].
///
/// Mirrors SQLite's dynamic typing: `NULL`, integer, float, text, blob.
#[derive(Debug, Clone, PartialEq)]
pub enum SqliteValue {
    /// SQL `NULL`.
    Null,
    /// 64-bit signed integer.
    Integer(i64),
    /// 64-bit IEEE float.
    Float(f64),
    /// UTF-8 text.
    Text(String),
    /// Raw bytes.
    Blob(Vec<u8>),
}

impl SqliteValue {
    /// Borrow the text payload when this value is `Text`.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Copy the integer payload when this value is `Integer`.
    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }
}

impl From<&str> for SqliteValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for SqliteValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<i64> for SqliteValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for SqliteValue {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<u32> for SqliteValue {
    fn from(value: u32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<f64> for SqliteValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for SqliteValue {
    fn from(value: bool) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<Vec<u8>> for SqliteValue {
    fn from(value: Vec<u8>) -> Self {
        Self::Blob(value)
    }
}

impl From<&[u8]> for SqliteValue {
    fn from(value: &[u8]) -> Self {
        Self::Blob(value.to_vec())
    }
}

impl From<ValueRef<'_>> for SqliteValue {
    fn from(value: ValueRef<'_>) -> Self {
        match value {
            ValueRef::Null => Self::Null,
            ValueRef::Integer(raw) => Self::Integer(raw),
            ValueRef::Real(raw) => Self::Float(raw),
            ValueRef::Text(raw) => Self::Text(String::from_utf8_lossy(raw).into_owned()),
            ValueRef::Blob(raw) => Self::Blob(raw.to_vec()),
        }
    }
}

impl ToSql for SqliteValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Self::Null => ToSqlOutput::Owned(rusqlite::types::Value::Null),
            Self::Integer(raw) => ToSqlOutput::Owned(rusqlite::types::Value::Integer(*raw)),
            Self::Float(raw) => ToSqlOutput::Owned(rusqlite::types::Value::Real(*raw)),
            Self::Text(text) => ToSqlOutput::Borrowed(ValueRef::Text(text.as_bytes())),
            Self::Blob(bytes) => ToSqlOutput::Borrowed(ValueRef::Blob(bytes)),
        })
    }
}

/// One result row, buffered so it outlives the statement that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    cells: Vec<SqliteValue>,
}

impl Row {
    /// Borrow the cell at `index`, or `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&SqliteValue> {
        self.cells.get(index)
    }

    /// All cells in column order.
    #[must_use]
    pub fn values(&self) -> &[SqliteValue] {
        &self.cells
    }

    /// Number of columns in the row.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// True when the row has no columns (never produced by SQLite today).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

fn buffer_rows(stmt: &mut Statement<'_>, params: &[SqliteValue]) -> Result<Vec<Row>, RError> {
    let column_count = stmt.column_count();
    let mut rows = stmt.query(params_from_iter(params.iter()))?;
    let mut buffered = Vec::new();
    while let Some(row) = rows.next()? {
        let mut cells = Vec::with_capacity(column_count);
        for index in 0..column_count {
            cells.push(SqliteValue::from(row.get_ref(index)?));
        }
        buffered.push(Row { cells });
    }
    Ok(buffered)
}

fn exec_buffered(stmt: &mut Statement<'_>, params: &[SqliteValue]) -> Result<usize, RError> {
    match stmt.execute(params_from_iter(params.iter())) {
        Ok(changes) => Ok(changes),
        // PRAGMA assignments like `journal_mode = WAL` return a result row;
        // the pre-rusqlite engine tolerated executing them, so drain the
        // rows and report the connection's change counter instead of failing.
        Err(RError::ExecuteReturnedResults) => {
            let mut rows = stmt.query(params_from_iter(params.iter()))?;
            while rows.next()?.is_some() {}
            Ok(0)
        }
        Err(other) => Err(other),
    }
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

/// Synchronous SQLite connection over rusqlite.
///
/// Closing keeps the handle available for retry (`close_in_place`) the way
/// the previous facade did, so the wrapper owns an [`Option`]; every method
/// fails with [`DbError::Internal`] if the connection was already closed.
pub struct Connection {
    inner: Option<rusqlite::Connection>,
    path: String,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("path", &self.path)
            .field("closed", &self.inner.is_none())
            .finish()
    }
}

impl Connection {
    fn conn(&self) -> Result<&rusqlite::Connection, DbError> {
        self.inner
            .as_ref()
            .ok_or_else(|| DbError::Internal("connection already closed".to_string()))
    }

    /// Map a rusqlite failure through [`DbError::classify`] with this
    /// connection's path as context.
    pub(crate) fn db_error(&self, err: RError) -> DbError {
        DbError::classify(err, Some(&self.path))
    }

    /// Open (or create) a database at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::CannotOpen`] when the file cannot be opened or
    /// created (including the not-a-database case).
    pub fn open(path: impl Into<String>) -> Result<Self, DbError> {
        let path = path.into();
        Self::open_with_flags(&path, ROpenFlags::default())
    }

    /// Shared open path: installs the default busy handler and records the
    /// path for error classification.
    fn open_with_flags(path: &str, flags: ROpenFlags) -> Result<Self, DbError> {
        let inner = rusqlite::Connection::open_with_flags(path, flags)
            .map_err(|err| DbError::classify(err, Some(path)))?;
        inner
            .busy_timeout(DEFAULT_BUSY_TIMEOUT)
            .map_err(|err| DbError::classify(err, Some(path)))?;
        Ok(Self {
            inner: Some(inner),
            path: path.to_string(),
        })
    }

    /// Execute a single SQL statement, returning the affected row count.
    ///
    /// Statements that return rows (PRAGMA assignments) are drained instead
    /// of rejected, preserving the previous engine's tolerance.
    ///
    /// # Errors
    ///
    /// Returns any engine failure classified into [`DbError`].
    pub fn execute(&self, sql: &str) -> Result<usize, DbError> {
        self.execute_with_params(sql, &[])
    }

    /// Execute a single SQL statement with positional parameters.
    ///
    /// # Errors
    ///
    /// Returns any engine failure classified into [`DbError`].
    pub fn execute_with_params(&self, sql: &str, params: &[SqliteValue]) -> Result<usize, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(sql).map_err(|err| self.db_error(err))?;
        exec_buffered(&mut stmt, params).map_err(|err| self.db_error(err))
    }

    /// Query, returning all rows buffered out of the statement's lifetime.
    ///
    /// # Errors
    ///
    /// Returns any engine failure classified into [`DbError`].
    pub fn query(&self, sql: &str) -> Result<Vec<Row>, DbError> {
        self.query_with_params(sql, &[])
    }

    /// Query with positional parameters, returning all rows.
    ///
    /// # Errors
    ///
    /// Returns any engine failure classified into [`DbError`].
    pub fn query_with_params(
        &self,
        sql: &str,
        params: &[SqliteValue],
    ) -> Result<Vec<Row>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(sql).map_err(|err| self.db_error(err))?;
        buffer_rows(&mut stmt, params).map_err(|err| self.db_error(err))
    }

    /// Query, returning exactly one row.
    ///
    /// Zero rows yield [`DbError::QueryReturnedNoRows`]; more than one yields
    /// [`DbError::QueryReturnedMultipleRows`] — the strictness the JSONL
    /// recovery probe depends on.
    ///
    /// # Errors
    ///
    /// See above; other engine failures classify normally.
    pub fn query_row(&self, sql: &str) -> Result<Row, DbError> {
        self.query_row_with_params(sql, &[])
    }

    /// Query with positional parameters, returning exactly one row.
    ///
    /// # Errors
    ///
    /// Same strictness as [`Self::query_row`].
    pub fn query_row_with_params(&self, sql: &str, params: &[SqliteValue]) -> Result<Row, DbError> {
        let mut rows = self.query_with_params(sql, params)?;
        match rows.len() {
            0 => Err(DbError::QueryReturnedNoRows),
            1 => Ok(rows.remove(0)),
            _ => Err(DbError::QueryReturnedMultipleRows),
        }
    }

    /// Prepare a statement for repeated execution.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Engine`] (or a classified shape) when the SQL
    /// cannot be compiled.
    pub fn prepare(&self, sql: &str) -> Result<PreparedStatement<'_>, DbError> {
        let conn = self.conn()?;
        let inner = conn.prepare(sql).map_err(|err| self.db_error(err))?;
        Ok(PreparedStatement {
            conn: self,
            sql: sql.to_string(),
            inner,
        })
    }

    /// Last-inserted rowid on this connection.
    #[must_use]
    pub fn last_insert_rowid(&self) -> i64 {
        self.inner
            .as_ref()
            .map_or(0, rusqlite::Connection::last_insert_rowid)
    }

    /// Close the connection (rolling back any active transaction).
    ///
    /// # Errors
    ///
    /// Returns the classified close failure; the handle is retained on error
    /// so the caller can retry via [`Self::close_in_place`].
    pub fn close(mut self) -> Result<(), DbError> {
        self.close_in_place()
    }

    /// Close in place, retaining the handle on error so callers can retry.
    ///
    /// # Errors
    ///
    /// Returns the classified close failure.
    pub fn close_in_place(&mut self) -> Result<(), DbError> {
        let Some(conn) = self.inner.take() else {
            return Ok(());
        };
        match conn.close() {
            Ok(()) => Ok(()),
            Err((conn, err)) => {
                self.inner = Some(conn);
                Err(self.db_error(err))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Prepared statements
// ---------------------------------------------------------------------------

/// Prepared statement tied to the lifetime of its [`Connection`].
pub struct PreparedStatement<'conn> {
    conn: &'conn Connection,
    sql: String,
    inner: Statement<'conn>,
}

impl PreparedStatement<'_> {
    /// Render diagnostics for this statement.
    ///
    /// The previous engine printed its compiled program here; rusqlite gives
    /// no program text, so diagnostics render the statement SQL itself (the
    /// only caller-facing contract is that the output is non-empty lines).
    #[must_use]
    pub fn explain(&self) -> String {
        format!("prepared SQL:\n{}", self.sql)
    }

    fn run_query(&mut self, params: &[SqliteValue]) -> Result<Vec<Row>, DbError> {
        buffer_rows(&mut self.inner, params).map_err(|err| self.conn.db_error(err))
    }

    /// Query, returning all rows.
    ///
    /// # Errors
    ///
    /// Classified engine failure.
    pub fn query(&mut self) -> Result<Vec<Row>, DbError> {
        self.run_query(&[])
    }

    /// Query with positional parameters, returning all rows.
    ///
    /// # Errors
    ///
    /// Classified engine failure.
    pub fn query_with_params(&mut self, params: &[SqliteValue]) -> Result<Vec<Row>, DbError> {
        self.run_query(params)
    }

    /// Query, returning exactly one row (strict; see [`Connection::query_row`]).
    ///
    /// # Errors
    ///
    /// Same strictness as [`Connection::query_row`].
    pub fn query_row(&mut self) -> Result<Row, DbError> {
        self.query_row_with_params(&[])
    }

    /// Query with positional parameters, returning exactly one row.
    ///
    /// # Errors
    ///
    /// Same strictness as [`Connection::query_row`].
    pub fn query_row_with_params(&mut self, params: &[SqliteValue]) -> Result<Row, DbError> {
        let mut rows = self.run_query(params)?;
        match rows.len() {
            0 => Err(DbError::QueryReturnedNoRows),
            1 => Ok(rows.remove(0)),
            _ => Err(DbError::QueryReturnedMultipleRows),
        }
    }

    /// Execute, returning the affected row count.
    ///
    /// # Errors
    ///
    /// Classified engine failure.
    pub fn execute(&mut self) -> Result<usize, DbError> {
        self.execute_with_params(&[])
    }

    /// Execute with positional parameters, returning the affected row count.
    ///
    /// # Errors
    ///
    /// Classified engine failure.
    pub fn execute_with_params(&mut self, params: &[SqliteValue]) -> Result<usize, DbError> {
        exec_buffered(&mut self.inner, params).map_err(|err| self.conn.db_error(err))
    }
}

// ---------------------------------------------------------------------------
// compat: rusqlite-style open flags
// ---------------------------------------------------------------------------

pub mod compat {
    //! Open-flag compatibility surface for callers that open databases with
    //! explicit flags (read-only probes, reconcile writers).

    use super::{Connection, DbError};

    pub use rusqlite::OpenFlags;

    /// Open a database with explicit open flags.
    ///
    /// # Errors
    ///
    /// Classified engine failure.
    pub fn open_with_flags(path: &str, flags: OpenFlags) -> Result<Connection, DbError> {
        Connection::open_with_flags(path, flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_execute_query_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("roundtrip.db");
        let conn = Connection::open(db.to_string_lossy().into_owned()).expect("open");
        conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            .expect("create table");
        let inserted = conn
            .execute_with_params(
                "INSERT INTO t (v) VALUES (?1)",
                &[SqliteValue::from("hello")],
            )
            .expect("insert row");
        assert_eq!(inserted, 1);
        let rows = conn.query("SELECT v FROM t").expect("query rows");
        assert_eq!(rows.len(), 1);
        let row = conn
            .query_row_with_params("SELECT v FROM t WHERE id = ?1", &[SqliteValue::from(1i64)])
            .expect("query row");
        assert_eq!(row.get(0).and_then(SqliteValue::as_text), Some("hello"));
        conn.close().expect("close");
    }

    #[test]
    fn prepared_statement_roundtrip() {
        let conn = Connection::open(":memory:").expect("open");
        conn.execute("CREATE TABLE t (k TEXT)").expect("create");
        conn.execute_with_params("INSERT INTO t (k) VALUES (?1)", &[SqliteValue::from("a")])
            .expect("insert");
        let mut stmt = conn
            .prepare("SELECT count(*) FROM t WHERE k = ?1")
            .expect("prepare");
        assert!(!stmt.explain().is_empty(), "explain renders diagnostics");
        let row = stmt
            .query_row_with_params(&[SqliteValue::from("a")])
            .expect("query");
        assert_eq!(row.get(0).and_then(SqliteValue::as_integer), Some(1));
    }

    #[test]
    fn exec_tolerates_row_returning_pragma_and_wal_applies() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("wal.db");
        let conn = Connection::open(db.to_string_lossy().into_owned()).expect("open");
        conn.execute("PRAGMA journal_mode = WAL")
            .expect("PRAGMA assignment returns a row and must not fail execute");
        let mode = conn
            .query_row("PRAGMA journal_mode")
            .expect("read journal mode");
        assert_eq!(
            mode.get(0).and_then(SqliteValue::as_text),
            Some("wal"),
            "WAL mode must stick"
        );
    }

    #[test]
    fn default_busy_timeout_is_configured() {
        let conn = Connection::open(":memory:").expect("open");
        let row = conn.query_row("PRAGMA busy_timeout").expect("pragma");
        assert_eq!(row.get(0).and_then(SqliteValue::as_integer), Some(5000));
    }

    #[test]
    fn unique_violation_classifies_with_columns() {
        let conn = Connection::open(":memory:").expect("open");
        conn.execute("CREATE TABLE t (k TEXT UNIQUE)")
            .expect("create");
        conn.execute_with_params("INSERT INTO t (k) VALUES (?1)", &[SqliteValue::from("a")])
            .expect("first insert");
        let err = conn
            .execute_with_params("INSERT INTO t (k) VALUES (?1)", &[SqliteValue::from("a")])
            .expect_err("duplicate insert must fail");
        match err {
            DbError::UniqueViolation { columns } => {
                assert_eq!(columns, "t.k");
            }
            other => panic!("expected UniqueViolation, got {other:?}"),
        }
    }

    #[test]
    fn query_row_is_strict_about_row_count() {
        let conn = Connection::open(":memory:").expect("open");
        conn.execute("CREATE TABLE t (k INTEGER)").expect("create");
        let none = conn.query_row("SELECT k FROM t").expect_err("no rows");
        assert!(matches!(none, DbError::QueryReturnedNoRows));
        conn.execute("INSERT INTO t (k) VALUES (1), (2)")
            .expect("seed two rows");
        let many = conn.query_row("SELECT k FROM t").expect_err("two rows");
        assert!(matches!(many, DbError::QueryReturnedMultipleRows));
    }

    #[test]
    fn read_only_open_of_missing_file_maps_to_cannot_open() {
        let err = compat::open_with_flags(
            "/definitely/not/a/real/path/beads-missing.db",
            compat::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect_err("missing file must fail");
        assert!(
            matches!(err, DbError::CannotOpen { .. }),
            "expected CannotOpen, got {err:?}"
        );
    }
}
