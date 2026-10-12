use super::{
    CancelToken, ConnectionConfig, DbError, InterruptWatch, QueryResult, ResultBudget, ResultColumn,
};
use std::time::{Duration, Instant};

pub(super) fn open_sqlite(config: &ConnectionConfig) -> Result<rusqlite::Connection, DbError> {
    use rusqlite::{
        OpenFlags,
        hooks::{AuthAction, AuthContext, Authorization},
    };
    let flags = if config.read_only && config.database != ":memory:" {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let connection = rusqlite::Connection::open_with_flags(
        &config.database,
        flags | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    if config.read_only {
        connection.execute_batch("PRAGMA query_only = ON;")?;
        connection.authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Select
            | AuthAction::Read { .. }
            | AuthAction::Function { .. }
            | AuthAction::Recursive
            | AuthAction::Transaction { .. }
            | AuthAction::Savepoint { .. } => Authorization::Allow,
            AuthAction::Pragma { pragma_name, .. }
                if matches!(
                    pragma_name.to_ascii_lowercase().as_str(),
                    "table_info"
                        | "table_xinfo"
                        | "index_list"
                        | "index_info"
                        | "foreign_key_list"
                        | "database_list"
                ) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }));
    }
    Ok(connection)
}

pub(super) fn open_duckdb(config: &ConnectionConfig) -> Result<duckdb::Connection, DbError> {
    if config.read_only && config.database == ":memory:" {
        return Err(DbError(
            "DuckDB read-only mode requires an existing database file.".into(),
        ));
    }
    let options = duckdb::Config::default()
        .access_mode(if config.read_only {
            duckdb::AccessMode::ReadOnly
        } else {
            duckdb::AccessMode::ReadWrite
        })?
        .max_memory("1GB")?;
    Ok(duckdb::Connection::open_with_flags(
        &config.database,
        options,
    )?)
}

pub(super) fn sqlite_query(
    connection: &rusqlite::Connection,
    sql: &str,
    budget: &mut ResultBudget,
    cancel: &CancelToken,
    timeout: Duration,
) -> Result<QueryResult, DbError> {
    let handle = connection.get_interrupt_handle();
    let started = Instant::now();
    let _watch = InterruptWatch::start(cancel.clone(), timeout, move || handle.interrupt());
    let mut statement = connection.prepare(sql)?;
    let columns = statement
        .columns()
        .iter()
        .map(|column| ResultColumn {
            name: column.name().to_owned(),
            data_type: column.decl_type().unwrap_or("expression").to_owned(),
        })
        .collect();
    let mut result = QueryResult {
        columns,
        ..Default::default()
    };
    budget.columns(&result.columns)?;
    if result.columns.is_empty() {
        let changed = statement.execute([])? as u64;
        // SQLite leaves changes() unchanged after DDL and transaction control.
        // Only commands capable of direct row changes report that counter.
        let keyword = super::script::first_word(sql, super::Engine::SQLite)?;
        if matches!(
            keyword.as_str(),
            "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "WITH"
        ) {
            result.affected_rows = changed;
        }
    } else {
        let read_only = statement.readonly();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            if started.elapsed() >= timeout {
                return Err(DbError("Query timed out after 60 seconds.".into()));
            }
            let estimated = (0..result.columns.len()).try_fold(0_usize, |bytes, index| {
                row.get_ref(index)
                    .map(|value| bytes.saturating_add(sqlite_size(value)))
            })?;
            if !budget.can_materialize(&mut result, estimated, row.as_ref().column_count()) {
                if read_only {
                    break;
                }
                continue;
            }
            let cells = (0..result.columns.len())
                .map(|index| row.get_ref(index).map(sqlite_value))
                .collect::<Result<Vec<_>, _>>()?;
            budget.push(&mut result, cells);
            // SQLite finalizes the statement on drop. No unbounded client-side
            // result is materialized when the display limit is reached.
            if result.truncated && read_only {
                break;
            }
        }
        drop(rows);
        if !read_only {
            result.affected_rows = connection.changes();
        }
    }
    Ok(result)
}

fn sqlite_size(value: rusqlite::types::ValueRef<'_>) -> usize {
    match value {
        rusqlite::types::ValueRef::Null => 0,
        rusqlite::types::ValueRef::Text(value) => {
            if std::str::from_utf8(value).is_ok() {
                value.len()
            } else {
                value.len().saturating_mul(3)
            }
        }
        rusqlite::types::ValueRef::Blob(value) => value.len().saturating_mul(2).saturating_add(2),
        _ => 64,
    }
}

fn sqlite_value(value: rusqlite::types::ValueRef<'_>) -> Option<String> {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(value.to_string()),
        ValueRef::Real(value) => Some(value.to_string()),
        ValueRef::Text(value) => Some(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Some(hex_blob(value)),
    }
}

pub(super) fn hex_blob(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(value.len().saturating_mul(2).saturating_add(2));
    output.push_str("0x");
    for byte in value {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 15)] as char);
    }
    output
}

pub(super) fn duckdb_query(
    connection: &duckdb::Connection,
    sql: &str,
    budget: &mut ResultBudget,
    cancel: &CancelToken,
    timeout: Duration,
) -> Result<QueryResult, DbError> {
    use duckdb::arrow::{array::Array, util::display::array_value_to_string};
    let handle = connection.interrupt_handle();
    let _watch = InterruptWatch::start(cancel.clone(), timeout, move || handle.interrupt());
    let (command, returning) = super::script::command_and_returning(sql, super::Engine::DuckDB)?;
    let mutating = matches!(
        command.as_str(),
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "COPY"
    );
    let mut statement = connection.prepare(sql)?;
    let mut batches = statement.stream_arrow([])?;
    let columns = batches
        .get_schema()
        .fields()
        .iter()
        .map(|field| ResultColumn {
            name: field.name().clone(),
            data_type: field.data_type().to_string(),
        })
        .collect();
    let mut result = QueryResult {
        columns,
        ..Default::default()
    };
    budget.columns(&result.columns)?;
    let mut returned_rows = 0_u64;
    loop {
        cancel.check()?;
        // The driver's Arrow iterator panics on fetch errors. Convert that
        // documented driver failure into the application's normal error path.
        let batch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| batches.next()))
            .map_err(|_| {
                DbError(
                    "DuckDB could not fetch a result batch; the query may have been cancelled."
                        .into(),
                )
            })?;
        let Some(batch) = batch else {
            break;
        };
        for index in 0..batch.num_rows() {
            cancel.check()?;
            returned_rows = returned_rows.saturating_add(1);
            let estimated = batch.columns().iter().fold(0_usize, |bytes, column| {
                bytes.saturating_add(arrow_cell_size(column.as_ref(), index))
            });
            if !budget.can_materialize(&mut result, estimated, batch.num_columns()) {
                if !mutating {
                    break;
                }
                continue;
            }
            let row = batch
                .columns()
                .iter()
                .map(|column| {
                    if column.is_null(index) {
                        Ok(None)
                    } else {
                        array_value_to_string(column.as_ref(), index)
                            .map(Some)
                            .map_err(|error| DbError(error.to_string()))
                    }
                })
                .collect::<Result<Vec<_>, DbError>>()?;
            budget.push(&mut result, row);
            if result.truncated && !mutating {
                break;
            }
        }
        if result.truncated && !mutating {
            break;
        }
    }
    if mutating && returning {
        result.affected_rows = returned_rows;
    }
    if mutating && !returning && result.columns.len() == 1 && result.columns[0].name == "Count" {
        result.affected_rows = result
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(Option::as_ref)
            .and_then(|count| count.parse().ok())
            .unwrap_or(0);
        result.columns.clear();
        result.rows.clear();
    }
    Ok(result)
}

fn arrow_cell_size(array: &dyn duckdb::arrow::array::Array, index: usize) -> usize {
    use duckdb::arrow::{
        array::{
            BinaryArray, BinaryViewArray, LargeBinaryArray, LargeStringArray, StringArray,
            StringViewArray,
        },
        datatypes::DataType,
    };
    if array.is_null(index) {
        return 0;
    }
    match array.data_type() {
        DataType::Utf8 => array
            .as_any()
            .downcast_ref::<StringArray>()
            .map_or(usize::MAX, |array| array.value(index).len()),
        DataType::LargeUtf8 => array
            .as_any()
            .downcast_ref::<LargeStringArray>()
            .map_or(usize::MAX, |array| array.value(index).len()),
        DataType::Utf8View => array
            .as_any()
            .downcast_ref::<StringViewArray>()
            .map_or(usize::MAX, |array| array.value(index).len()),
        DataType::Binary => array
            .as_any()
            .downcast_ref::<BinaryArray>()
            .map_or(usize::MAX, |array| {
                array.value(index).len().saturating_mul(2)
            }),
        DataType::LargeBinary => array
            .as_any()
            .downcast_ref::<LargeBinaryArray>()
            .map_or(usize::MAX, |array| {
                array.value(index).len().saturating_mul(2)
            }),
        DataType::BinaryView => array
            .as_any()
            .downcast_ref::<BinaryViewArray>()
            .map_or(usize::MAX, |array| {
                array.value(index).len().saturating_mul(2)
            }),
        DataType::FixedSizeBinary(width) => usize::try_from(*width)
            .unwrap_or(usize::MAX)
            .saturating_mul(2),
        DataType::Dictionary(_, _) => array
            .slice(index, 1)
            .get_buffer_memory_size()
            .saturating_mul(4)
            .saturating_add(256),
        DataType::List(_)
        | DataType::LargeList(_)
        | DataType::FixedSizeList(_, _)
        | DataType::Struct(_)
        | DataType::Map(_, _)
        | DataType::Union(_, _) => {
            // Nested Arrow display can expand numbers and delimiters. A slice
            // shares source buffers, so this conservative bound allocates none.
            array
                .slice(index, 1)
                .get_buffer_memory_size()
                .saturating_mul(4)
                .saturating_add(256)
        }
        _ => 256,
    }
}
