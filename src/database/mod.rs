//! Database sessions own their driver connection and are shared with background jobs.
//! Query text is deliberately never logged, and passwords are excluded from workspace persistence.

mod embedded;
mod schema;
mod script;
mod server;
#[cfg(test)]
mod tests;

pub use schema::{ForeignKeyDetails, IndexDetails, TableDetails};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_ROWS: usize = 10_000;
const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;

/// EXPLAIN accepts exactly one statement so a second command can never be
/// executed accidentally by the query-plan action.
pub fn explain_sql(sql: &str, engine: Engine) -> Result<String, DbError> {
    let statements = script::split(sql, engine)?;
    if statements.len() != 1 {
        return Err(DbError(
            "Explain requires exactly one SQL statement.".into(),
        ));
    }
    let prefix = if engine == Engine::SQLite {
        "EXPLAIN QUERY PLAN"
    } else {
        "EXPLAIN"
    };
    Ok(format!("{prefix} {}", statements[0]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    SQLite,
    DuckDB,
    Postgres,
    MySql,
}

impl Engine {
    pub const ALL: [Self; 4] = [Self::SQLite, Self::DuckDB, Self::Postgres, Self::MySql];
    pub fn name(self) -> &'static str {
        match self {
            Self::SQLite => "SQLite",
            Self::DuckDB => "DuckDB",
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL",
        }
    }
    pub fn quote_identifier(self, name: &str) -> String {
        let quote = if self == Self::MySql { '`' } else { '"' };
        format!(
            "{quote}{}{quote}",
            name.replace(quote, &format!("{quote}{quote}"))
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub engine: Engine,
    pub database: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(skip)]
    pub password: String,
    #[serde(default)]
    pub remember_password: bool,
    pub read_only: bool,
    pub tls: bool,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Local SQLite".into(),
            engine: Engine::SQLite,
            database: ":memory:".into(),
            host: "localhost".into(),
            port: 5432,
            user: String::new(),
            password: String::new(),
            remember_password: true,
            read_only: true,
            tls: true,
        }
    }
}

impl fmt::Debug for ConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionConfig")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("engine", &self.engine)
            .field("database", &self.database)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("read_only", &self.read_only)
            .field("tls", &self.tls)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
    Table,
    View,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbObject {
    pub schema: String,
    pub name: String,
    pub kind: ObjectKind,
}
#[derive(Clone, Debug)]
pub struct ResultColumn {
    pub name: String,
    pub data_type: String,
}
#[derive(Clone, Debug, Default)]
pub struct QueryResult {
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<Vec<Option<String>>>,
    pub affected_rows: u64,
    pub truncated: bool,
}
#[derive(Clone, Debug)]
pub struct QueryOutput {
    pub results: Vec<QueryResult>,
    pub elapsed_ms: u64,
}
#[derive(Clone, Debug)]
pub struct ColumnDetails {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
}
#[derive(Clone, Debug)]
pub struct DbError(pub String);
impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DbError {}
impl From<rusqlite::Error> for DbError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<duckdb::Error> for DbError {
    fn from(error: duckdb::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<tokio_postgres::Error> for DbError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self(
            error
                .as_db_error()
                .map_or_else(|| error.to_string(), |db| db.message().to_owned()),
        )
    }
}
impl From<mysql::Error> for DbError {
    fn from(error: mysql::Error) -> Self {
        Self(error.to_string())
    }
}

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn check(&self) -> Result<(), DbError> {
        if self.is_cancelled() {
            Err(DbError("Query cancelled.".into()))
        } else {
            Ok(())
        }
    }
}

enum Backend {
    SQLite(rusqlite::Connection),
    DuckDB(duckdb::Connection),
    Postgres(server::PostgresSession),
    MySql(server::MySqlSession),
}

#[derive(Clone)]
pub struct DatabaseSession {
    inner: Arc<Mutex<Backend>>,
    engine: Engine,
    read_only: bool,
}

impl DatabaseSession {
    pub fn connect(config: &ConnectionConfig) -> Result<Self, DbError> {
        if config.database.trim().is_empty() {
            return Err(DbError(
                "Database name or local file path is required.".into(),
            ));
        }
        if matches!(config.engine, Engine::Postgres | Engine::MySql)
            && (config.host.trim().is_empty() || config.port == 0)
        {
            return Err(DbError("Server host and a valid port are required.".into()));
        }
        let backend = match config.engine {
            Engine::SQLite => embedded::open_sqlite(config).map(Backend::SQLite),
            Engine::DuckDB => embedded::open_duckdb(config).map(Backend::DuckDB),
            Engine::Postgres => server::PostgresSession::connect(config).map(Backend::Postgres),
            Engine::MySql => server::MySqlSession::connect(config).map(Backend::MySql),
        }
        .map_err(|error| {
            if config.password.is_empty() {
                error
            } else {
                DbError(error.0.replace(&config.password, "[redacted]"))
            }
        })?;
        Ok(Self {
            inner: Arc::new(Mutex::new(backend)),
            engine: config.engine,
            read_only: config.read_only,
        })
    }

    pub fn quote_object(&self, object: &DbObject) -> String {
        let quote = |name: &str| self.engine.quote_identifier(name);
        if object.schema.is_empty() {
            quote(&object.name)
        } else {
            format!("{}.{}", quote(&object.schema), quote(&object.name))
        }
    }

    pub fn execute(
        &self,
        sql: &str,
        row_limit: usize,
        cancel: &CancelToken,
    ) -> Result<QueryOutput, DbError> {
        cancel.check()?;
        let statements = script::split(sql, self.engine)?;
        if statements.is_empty() {
            return Err(DbError("Enter a SQL statement before running.".into()));
        }
        let started = Instant::now();
        let mut backend = self.inner.try_lock().map_err(|_| {
            DbError("Connection is busy. Wait for the current operation to finish.".into())
        })?;
        let mut budget = ResultBudget {
            row_limit: row_limit.clamp(1, MAX_ROWS),
            remaining_bytes: MAX_RESULT_BYTES,
        };
        let mut results = Vec::with_capacity(statements.len());
        for statement in statements {
            cancel.check()?;
            if started.elapsed() >= QUERY_TIMEOUT {
                return Err(DbError("Query timed out after 60 seconds.".into()));
            }
            let remaining = QUERY_TIMEOUT.saturating_sub(started.elapsed());
            let statement_results = match &mut *backend {
                Backend::SQLite(connection) => {
                    vec![embedded::sqlite_query(
                        connection,
                        &statement,
                        &mut budget,
                        cancel,
                        remaining,
                    )?]
                }
                Backend::DuckDB(connection) => {
                    vec![embedded::duckdb_query(
                        connection,
                        &statement,
                        &mut budget,
                        cancel,
                        remaining,
                    )?]
                }
                Backend::Postgres(connection) => {
                    vec![connection.query(
                        &statement,
                        &mut budget,
                        cancel,
                        remaining,
                        self.read_only,
                    )?]
                }
                Backend::MySql(connection) => {
                    connection.query(&statement, &mut budget, cancel, remaining, self.read_only)?
                }
            };
            if results.len() + statement_results.len() > 1000 {
                return Err(DbError(
                    "A script can return at most 1,000 result sets.".into(),
                ));
            }
            results.extend(statement_results);
        }
        Ok(QueryOutput {
            results,
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        })
    }

    pub fn catalog(&self) -> Result<Vec<DbObject>, DbError> {
        let sql = match self.engine {
            Engine::SQLite => {
                "SELECT 'main', name, type FROM main.sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY type, name"
            }
            Engine::DuckDB => {
                "SELECT table_schema, table_name, CASE WHEN table_type = 'VIEW' THEN 'view' ELSE 'table' END FROM information_schema.tables WHERE table_catalog = current_database() AND table_schema NOT IN ('information_schema','pg_catalog') ORDER BY table_schema, table_name"
            }
            Engine::Postgres => {
                "SELECT table_schema, table_name, CASE WHEN table_type = 'VIEW' THEN 'view' ELSE 'table' END FROM information_schema.tables WHERE table_schema NOT IN ('information_schema','pg_catalog') ORDER BY table_schema, table_name"
            }
            Engine::MySql => {
                "SELECT TABLE_SCHEMA, TABLE_NAME, CASE WHEN TABLE_TYPE = 'VIEW' THEN 'view' ELSE 'table' END FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME"
            }
        };
        let output = self.execute(sql, MAX_ROWS, &CancelToken::new())?;
        let result = output
            .results
            .first()
            .ok_or_else(|| DbError("Catalog query returned no result.".into()))?;
        if result.truncated {
            return Err(DbError(
                "Catalog exceeds the 10,000 object limit. Narrow the database scope.".into(),
            ));
        }
        Ok(result
            .rows
            .iter()
            .map(|row| DbObject {
                schema: cell(row, 0),
                name: cell(row, 1),
                kind: if cell(row, 2) == "view" {
                    ObjectKind::View
                } else {
                    ObjectKind::Table
                },
            })
            .collect())
    }

    pub fn describe(&self, object: &DbObject) -> Result<Vec<ColumnDetails>, DbError> {
        let literal = |text: &str| match self.engine {
            Engine::MySql => format!(
                "CONVERT(X'{}' USING utf8mb4)",
                text.as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<String>()
            ),
            Engine::Postgres => format!("E'{}'", text.replace('\\', "\\\\").replace('\'', "''")),
            _ => format!("'{}'", text.replace('\'', "''")),
        };
        let sql = if self.engine == Engine::SQLite {
            format!(
                "PRAGMA {}.table_info({})",
                self.engine.quote_identifier(&object.schema),
                self.engine.quote_identifier(&object.name)
            )
        } else {
            // Catalog values are data, not SQL. Each literal is escaped independently.
            let name = literal(&object.name);
            let schema = literal(&object.schema);
            let catalog_filter = if self.engine == Engine::DuckDB {
                " AND c.table_catalog = current_database()"
            } else {
                ""
            };
            format!(
                "SELECT c.column_name, c.data_type, c.is_nullable, CASE WHEN EXISTS (SELECT 1 FROM information_schema.table_constraints t JOIN information_schema.key_column_usage k ON t.constraint_name = k.constraint_name AND t.table_catalog = k.table_catalog AND t.table_schema = k.table_schema AND t.table_name = k.table_name WHERE t.constraint_type = 'PRIMARY KEY' AND k.table_catalog = c.table_catalog AND k.table_schema = c.table_schema AND k.table_name = c.table_name AND k.column_name = c.column_name) THEN 'YES' ELSE 'NO' END FROM information_schema.columns c WHERE c.table_schema = {schema} AND c.table_name = {name}{catalog_filter} ORDER BY c.ordinal_position"
            )
        };
        let output = self.execute(&sql, MAX_ROWS, &CancelToken::new())?;
        let result = output
            .results
            .first()
            .ok_or_else(|| DbError("Column query returned no result.".into()))?;
        if result.truncated {
            return Err(DbError("Column metadata exceeds the display limit.".into()));
        }
        Ok(result
            .rows
            .iter()
            .map(|row| {
                if self.engine == Engine::SQLite {
                    let primary_key = cell(row, 5) != "0";
                    ColumnDetails {
                        name: cell(row, 1),
                        data_type: cell(row, 2),
                        nullable: cell(row, 3) == "0",
                        primary_key,
                    }
                } else {
                    ColumnDetails {
                        name: cell(row, 0),
                        data_type: cell(row, 1),
                        nullable: cell(row, 2) == "YES",
                        primary_key: cell(row, 3) == "YES",
                    }
                }
            })
            .collect())
    }
}

fn cell(row: &[Option<String>], index: usize) -> String {
    row.get(index)
        .and_then(|value| value.clone())
        .unwrap_or_default()
}

struct ResultBudget {
    row_limit: usize,
    remaining_bytes: usize,
}
impl ResultBudget {
    fn columns(&mut self, columns: &[ResultColumn]) -> Result<(), DbError> {
        if columns.len() > 1024 {
            return Err(DbError("Results are limited to 1,024 columns.".into()));
        }
        let bytes = std::mem::size_of::<QueryResult>()
            + std::mem::size_of_val(columns)
            + columns
                .iter()
                .map(|column| column.name.capacity() + column.data_type.capacity())
                .sum::<usize>();
        if bytes > self.remaining_bytes {
            return Err(DbError(
                "Result metadata exceeds the 16 MiB display budget.".into(),
            ));
        }
        self.remaining_bytes -= bytes;
        Ok(())
    }

    fn push(&mut self, result: &mut QueryResult, row: Vec<Option<String>>) {
        if result.truncated {
            return;
        }
        let bytes = std::mem::size_of::<Vec<Option<String>>>()
            + row.capacity() * std::mem::size_of::<Option<String>>()
            + row
                .iter()
                .filter_map(Option::as_ref)
                .map(String::capacity)
                .sum::<usize>();
        if result.rows.len() < self.row_limit && bytes <= self.remaining_bytes {
            self.remaining_bytes -= bytes;
            result.rows.push(row);
        } else {
            result.truncated = true;
        }
    }

    fn can_materialize(
        &self,
        result: &mut QueryResult,
        cell_bytes: usize,
        column_count: usize,
    ) -> bool {
        let bytes = std::mem::size_of::<Vec<Option<String>>>()
            .saturating_add(column_count.saturating_mul(std::mem::size_of::<Option<String>>()))
            .saturating_add(cell_bytes);
        if result.truncated || result.rows.len() >= self.row_limit || bytes > self.remaining_bytes {
            result.truncated = true;
            false
        } else {
            true
        }
    }
}

/// The watcher is joined before releasing the session lock, preventing a late
/// cancellation request from interrupting the next query on the same connection.
struct InterruptWatch {
    done: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl InterruptWatch {
    fn start(
        cancel: CancelToken,
        timeout: Duration,
        interrupt: impl FnOnce() + Send + 'static,
    ) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let thread_done = done.clone();
        let thread = std::thread::spawn(move || {
            let started = Instant::now();
            while !thread_done.load(Ordering::Acquire) {
                if cancel.is_cancelled() || started.elapsed() >= timeout {
                    interrupt();
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
        });
        Self {
            done,
            thread: Some(thread),
        }
    }
}
impl Drop for InterruptWatch {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}
