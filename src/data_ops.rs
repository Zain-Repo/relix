//! Reviewable data operations. Generated SQL is opened in the editor, never
//! executed as a side effect of selecting or importing a file.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::database::{ColumnDetails, DbObject, Engine, ObjectKind, QueryResult};

const MAX_IMPORT_BYTES: usize = 1024 * 1024;
const MAX_GENERATED_SQL_BYTES: usize = 512 * 1024;
const MAX_IMPORT_ROWS: usize = 10_000;
const MAX_MARKDOWN_BYTES: usize = 1024 * 1024;

/// Exports loaded rows as a Markdown table, retaining duplicate column names.
/// NULL is italicized; text cells escape Markdown and HTML syntax.
pub fn result_to_markdown(result: &QueryResult) -> Result<String, String> {
    validate_export_shape(result)?;
    let mut output = String::new();
    append_export(&mut output, "|", MAX_MARKDOWN_BYTES)?;
    for column in &result.columns {
        append_markdown_cell(&mut output, Some(&column.name))?;
    }
    append_export(&mut output, "\n|", MAX_MARKDOWN_BYTES)?;
    for _ in &result.columns {
        append_export(&mut output, " --- |", MAX_MARKDOWN_BYTES)?;
    }
    append_export(&mut output, "\n", MAX_MARKDOWN_BYTES)?;
    for row in &result.rows {
        append_export(&mut output, "|", MAX_MARKDOWN_BYTES)?;
        for cell in row {
            append_markdown_cell(&mut output, cell.as_deref())?;
        }
        append_export(&mut output, "\n", MAX_MARKDOWN_BYTES)?;
    }
    Ok(output)
}

/// Builds reviewable INSERT statements for loaded rows in an existing table.
/// Scalar display values are quoted text for the target column to convert.
/// Binary, compound and unknown types are rejected because display strings
/// cannot reliably reconstruct their original values. No SQL is executed here.
pub fn result_to_insert_sql(
    engine: Engine,
    object: &DbObject,
    result: &QueryResult,
) -> Result<String, String> {
    validate_export_shape(result)?;
    if object.kind != ObjectKind::Table {
        return Err("SQL INSERT export requires a table result.".into());
    }
    validate_identifier(&object.name)?;
    if !object.schema.is_empty() {
        validate_identifier(&object.schema)?;
    }
    let mut names = HashSet::new();
    for column in &result.columns {
        validate_identifier(&column.name)?;
        if !names.insert(column.name.to_lowercase()) {
            return Err("SQL INSERT export requires unique column names.".into());
        }
        if !supported_insert_type(&column.data_type) {
            return Err("SQL INSERT export supports scalar text, numeric, boolean and temporal columns only. Use CSV or JSON for other types.".into());
        }
    }
    let table = if object.schema.is_empty() {
        identifier(engine, &object.name)
    } else {
        format!(
            "{}.{}",
            identifier(engine, &object.schema),
            identifier(engine, &object.name)
        )
    };
    let columns = result
        .columns
        .iter()
        .map(|column| identifier(engine, &column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let prefix = format!("INSERT INTO {table} ({columns}) VALUES (");
    let mut sql = String::new();
    append_export(
        &mut sql,
        "-- Loaded-row INSERT preview. Review target columns and values before running.\n-- Scalar display values are converted by the target column types.\n",
        MAX_GENERATED_SQL_BYTES,
    )?;
    if result.truncated {
        append_export(
            &mut sql,
            "-- The source result was truncated; only loaded rows are included.\n",
            MAX_GENERATED_SQL_BYTES,
        )?;
    }
    for row in &result.rows {
        append_export(&mut sql, &prefix, MAX_GENERATED_SQL_BYTES)?;
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                append_export(&mut sql, ", ", MAX_GENERATED_SQL_BYTES)?;
            }
            match cell {
                Some(value) => {
                    // SQLite permits blobs in scalar-declared columns; MySQL
                    // also renders undecodable bytes with the same hex prefix.
                    // Display metadata cannot distinguish these from literal
                    // hex-looking text, so reject the ambiguous case safely.
                    if matches!(engine, Engine::SQLite | Engine::MySql)
                        && value.strip_prefix("0x").is_some_and(|hex| {
                            hex.len().is_multiple_of(2)
                                && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                    {
                        return Err("A hex-formatted value may represent binary data. Use CSV or JSON instead of SQL INSERT export.".into());
                    }
                    if value.len() > MAX_GENERATED_SQL_BYTES {
                        return Err("A value exceeds the SQL INSERT export size limit.".into());
                    }
                    append_export(
                        &mut sql,
                        &text_literal(engine, value)?,
                        MAX_GENERATED_SQL_BYTES,
                    )?;
                }
                None => append_export(&mut sql, "NULL", MAX_GENERATED_SQL_BYTES)?,
            }
        }
        append_export(&mut sql, ");\n", MAX_GENERATED_SQL_BYTES)?;
    }
    Ok(sql)
}

fn validate_export_shape(result: &QueryResult) -> Result<(), String> {
    if result.columns.is_empty() || result.columns.len() > 128 {
        return Err("Export requires between 1 and 128 result columns.".into());
    }
    if result.rows.len() > MAX_IMPORT_ROWS {
        return Err("Loaded-result exports are limited to 10,000 rows.".into());
    }
    if result
        .rows
        .iter()
        .any(|row| row.len() != result.columns.len())
    {
        return Err("A result row does not match its columns.".into());
    }
    Ok(())
}

fn append_export(output: &mut String, value: &str, limit: usize) -> Result<(), String> {
    if value.len() > limit.saturating_sub(output.len()) {
        return Err(format!(
            "The export exceeds {} KiB. Load fewer rows or columns.",
            limit / 1024
        ));
    }
    output.push_str(value);
    Ok(())
}

fn append_markdown_cell(output: &mut String, cell: Option<&str>) -> Result<(), String> {
    append_export(output, " ", MAX_MARKDOWN_BYTES)?;
    if let Some(value) = cell {
        let mut chars = value.chars().peekable();
        while let Some(ch) = chars.next() {
            let escaped = match ch {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '\\' => "\\\\",
                '|' => "\\|",
                '*' => "\\*",
                '_' => "\\_",
                '`' => "\\`",
                '[' => "\\[",
                ']' => "\\]",
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    "<br>"
                }
                '\n' => "<br>",
                _ => {
                    let mut buffer = [0; 4];
                    append_export(output, ch.encode_utf8(&mut buffer), MAX_MARKDOWN_BYTES)?;
                    continue;
                }
            };
            append_export(output, escaped, MAX_MARKDOWN_BYTES)?;
        }
    } else {
        append_export(output, "*NULL*", MAX_MARKDOWN_BYTES)?;
    }
    append_export(output, " |", MAX_MARKDOWN_BYTES)
}

fn supported_insert_type(data_type: &str) -> bool {
    let normalized = data_type.trim().to_ascii_uppercase();
    // Driver metadata includes SQL names, Arrow names and MySQL enum names.
    // Accept only explicitly recognized scalar families; never reinterpret blobs.
    if normalized.contains('[')
        || normalized.contains(']')
        || ["BLOB", "BYTEA", "BINARY", "STRUCT", "LIST", "MAP", "ARRAY"]
            .iter()
            .any(|kind| normalized.contains(kind))
    {
        return false;
    }
    let base = normalized.split(['(', ' ']).next().unwrap_or_default();
    matches!(
        base,
        "TEXT"
            | "VARCHAR"
            | "CHAR"
            | "BPCHAR"
            | "NAME"
            | "CLOB"
            | "LONGTEXT"
            | "MEDIUMTEXT"
            | "TINYTEXT"
            | "UTF8"
            | "LARGEUTF8"
            | "UTF8VIEW"
            | "BOOL"
            | "BOOLEAN"
            | "INTEGER"
            | "INT"
            | "INT2"
            | "INT4"
            | "INT8"
            | "INT16"
            | "INT32"
            | "INT64"
            | "UINT8"
            | "UINT16"
            | "UINT32"
            | "UINT64"
            | "SMALLINT"
            | "TINYINT"
            | "BIGINT"
            | "HUGEINT"
            | "REAL"
            | "FLOAT"
            | "FLOAT4"
            | "FLOAT8"
            | "FLOAT16"
            | "FLOAT32"
            | "FLOAT64"
            | "DOUBLE"
            | "NUMERIC"
            | "DECIMAL"
            | "DECIMAL128"
            | "DECIMAL256"
            | "DATE"
            | "DATE32"
            | "DATE64"
            | "TIME"
            | "TIME32"
            | "TIME64"
            | "TIMESTAMP"
            | "TIMESTAMPTZ"
            | "DATETIME"
            | "UUID"
            | "MYSQL_TYPE_VARCHAR"
            | "MYSQL_TYPE_VAR_STRING"
            | "MYSQL_TYPE_STRING"
            | "MYSQL_TYPE_TINY"
            | "MYSQL_TYPE_SHORT"
            | "MYSQL_TYPE_LONG"
            | "MYSQL_TYPE_LONGLONG"
            | "MYSQL_TYPE_INT24"
            | "MYSQL_TYPE_FLOAT"
            | "MYSQL_TYPE_DOUBLE"
            | "MYSQL_TYPE_DECIMAL"
            | "MYSQL_TYPE_NEWDECIMAL"
            | "MYSQL_TYPE_YEAR"
            | "MYSQL_TYPE_DATE"
            | "MYSQL_TYPE_NEWDATE"
            | "MYSQL_TYPE_TIME"
            | "MYSQL_TYPE_TIME2"
            | "MYSQL_TYPE_DATETIME"
            | "MYSQL_TYPE_DATETIME2"
            | "MYSQL_TYPE_TIMESTAMP"
            | "MYSQL_TYPE_TIMESTAMP2"
    )
}

pub fn result_to_csv(result: &QueryResult) -> Result<String, String> {
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer
        .write_record(result.columns.iter().map(|column| column.name.as_str()))
        .map_err(|_| "CSV headers could not be written.".to_string())?;
    for row in &result.rows {
        if row.len() != result.columns.len() {
            return Err("A result row does not match its columns.".into());
        }
        // CSV has no standard NULL type. JSON export retains NULL distinctly.
        writer
            .write_record(row.iter().map(|cell| cell.as_deref().unwrap_or_default()))
            .map_err(|_| "A CSV record could not be written.".to_string())?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|_| "CSV output could not be completed.".to_string())?;
    String::from_utf8(bytes).map_err(|_| "CSV output is not valid UTF-8.".into())
}

pub fn result_to_json(result: &QueryResult) -> Result<String, String> {
    if result
        .rows
        .iter()
        .any(|row| row.len() != result.columns.len())
    {
        return Err("A result row does not match its columns.".into());
    }
    // Ordered metadata and row arrays preserve duplicate column names and NULLs.
    let columns: Vec<_> = result
        .columns
        .iter()
        .map(|column| serde_json::json!({ "name": column.name, "data_type": column.data_type }))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({"columns": columns, "rows": result.rows}))
        .map_err(|_| "JSON output could not be serialized.".into())
}

pub fn import_csv_sql(engine: Engine, table: &str, source: &str) -> Result<String, String> {
    validate_identifier(table)?;
    if source.len() > MAX_IMPORT_BYTES {
        return Err(
            "CSV previews are limited to 1 MiB. Split this file into smaller imports.".into(),
        );
    }
    let mut reader = csv::ReaderBuilder::new()
        .flexible(false)
        .from_reader(source.trim_start_matches('\u{feff}').as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| "The CSV header could not be parsed.".to_string())?
        .clone();
    if headers.is_empty() || headers.len() > 128 {
        return Err("CSV requires between 1 and 128 named columns.".into());
    }
    let mut seen = HashSet::new();
    for header in &headers {
        validate_identifier(header)?;
        if !seen.insert(header.to_lowercase()) {
            return Err("CSV headers must be unique, including differences in case.".into());
        }
    }
    let quoted_table = identifier(engine, table);
    let columns = headers
        .iter()
        .map(|header| identifier(engine, header))
        .collect::<Vec<_>>();
    let definitions = columns
        .iter()
        .map(|column| {
            format!(
                "{column} {}",
                if engine == Engine::MySql {
                    "LONGTEXT"
                } else {
                    "TEXT"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "-- CSV import preview: creates a new table with TEXT columns.\n-- Review before running on a writable connection. Empty fields are empty text.\n-- MySQL CREATE TABLE commits implicitly; roll back inserts if a statement fails.\nBEGIN;\nCREATE TABLE {quoted_table} ({definitions});\n"
    );
    let mut count = 0;
    for record in reader.records() {
        let record = record.map_err(|_| {
            "A CSV record has invalid quoting or a different column count.".to_string()
        })?;
        count += 1;
        if count > MAX_IMPORT_ROWS {
            return Err("CSV previews are limited to 10,000 rows.".into());
        }
        let values = record
            .iter()
            .map(|value| text_literal(engine, value))
            .collect::<Result<Vec<_>, _>>()?;
        if count == 1 {
            writeln!(
                sql,
                "INSERT INTO {quoted_table} ({}) VALUES",
                columns.join(", ")
            )
            .map_err(|_| "The import preview could not be created.".to_string())?;
        } else {
            sql.push_str(",\n");
        }
        write!(sql, "({})", values.join(", "))
            .map_err(|_| "The import preview could not be created.".to_string())?;
        if sql.len() > MAX_GENERATED_SQL_BYTES {
            return Err(
                "The SQL preview exceeds 512 KiB. Split this file into smaller imports.".into(),
            );
        }
    }
    if count > 0 {
        sql.push_str(";\n");
    }
    sql.push_str("COMMIT;\n");
    Ok(sql)
}

/// Builds a single-row change with the primary key and original values in its
/// predicate. A zero-row update signals stale data rather than overwriting it.
pub fn edit_record_sql(
    engine: Engine,
    object: &DbObject,
    schema: &[ColumnDetails],
    result: &QueryResult,
    row_index: usize,
    column_index: usize,
    new_value: Option<&str>,
) -> Result<String, String> {
    if object.kind != ObjectKind::Table {
        return Err("Record editing is available for tables with a primary key.".into());
    }
    let row = result
        .rows
        .get(row_index)
        .ok_or("Select a result row to edit.")?;
    let column = result
        .columns
        .get(column_index)
        .ok_or("Select a result column to edit.")?;
    if row.len() != result.columns.len() {
        return Err("The selected row does not match its columns.".into());
    }
    let primary = schema
        .iter()
        .filter(|column| column.primary_key)
        .collect::<Vec<_>>();
    if primary.is_empty()
        || primary.iter().any(|key| {
            result
                .columns
                .iter()
                .filter(|column| column.name == key.name)
                .count()
                != 1
        })
    {
        return Err(
            "The result must include every primary-key column to edit a record safely.".into(),
        );
    }
    // SQLite rowid tables can have nullable primary keys. Such keys do not
    // identify one row, even when every displayed original value matches.
    for key in &primary {
        let index = result
            .columns
            .iter()
            .position(|column| column.name == key.name)
            .ok_or("The result is missing a primary-key column.")?;
        if row[index].is_none() {
            return Err("Record editing requires non-NULL primary-key values. Use explicit SQL for this row.".into());
        }
    }
    if !schema.iter().any(|field| field.name == column.name) {
        return Err("The selected column is not a table column.".into());
    }
    let mut predicates = Vec::new();
    for (index, result_column) in result.columns.iter().enumerate() {
        let Some(field) = schema.iter().find(|field| field.name == result_column.name) else {
            return Err(
                "Edit the table's own data query, rather than a computed or joined result.".into(),
            );
        };
        if ["BLOB", "BYTEA", "BINARY", "STRUCT", "LIST", "MAP", "ARRAY"]
            .iter()
            .any(|kind| field.data_type.to_uppercase().contains(kind))
        {
            return Err("Binary and compound records require an explicit SQL edit.".into());
        }
        let name = identifier(engine, &field.name);
        predicates.push(match &row[index] {
            Some(value) => format!("{name} = {}", text_literal(engine, value)?),
            None => format!("{name} IS NULL"),
        });
    }
    let value = new_value
        .map(|value| text_literal(engine, value))
        .transpose()?
        .unwrap_or_else(|| "NULL".into());
    let table = if object.schema.is_empty() {
        identifier(engine, &object.name)
    } else {
        format!(
            "{}.{}",
            identifier(engine, &object.schema),
            identifier(engine, &object.name)
        )
    };
    Ok(format!(
        "-- Record edit preview. Expect one affected row; zero means stale data.\nUPDATE {table}\nSET {} = {value}\nWHERE {};\n",
        identifier(engine, &column.name),
        predicates.join("\n  AND ")
    ))
}

/// An omitted column uses the database default; NULL and empty text are distinct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordValue {
    Default,
    Null,
    Text(String),
}

/// Builds one INSERT draft from actual table metadata without executing it.
/// Defaults and generated columns can be omitted; the database validates required
/// values and constraints when the user explicitly runs the reviewed statement.
pub fn insert_record_sql(
    engine: Engine,
    object: &DbObject,
    schema: &[ColumnDetails],
    values: &[RecordValue],
) -> Result<String, String> {
    let table = record_table(engine, object)?;
    if schema.is_empty() || schema.len() > 128 || values.len() != schema.len() {
        return Err("An insert requires matching metadata for 1–128 columns.".into());
    }
    let mut names = HashSet::new();
    let mut included = Vec::new();
    let mut literals = Vec::new();
    let mut total_bytes = 0usize;
    for (column, value) in schema.iter().zip(values) {
        validate_identifier(&column.name)?;
        if !names.insert(&column.name) {
            return Err("Table metadata contains duplicate column names.".into());
        }
        let literal = match value {
            RecordValue::Default => continue,
            RecordValue::Null => {
                if !column.nullable {
                    return Err(format!(
                        "{} does not allow SQL NULL. Omit it to use a database default or provide a value.",
                        column.name
                    ));
                }
                "NULL".into()
            }
            RecordValue::Text(value) => {
                if !supported_insert_type(&column.data_type) {
                    return Err(format!(
                        "{} requires explicit SQL for its binary, compound or unknown type.",
                        column.name
                    ));
                }
                if value.len() > MAX_GENERATED_SQL_BYTES {
                    return Err("A record value exceeds the SQL preview size limit.".into());
                }
                text_literal(engine, value)?
            }
        };
        total_bytes = total_bytes.saturating_add(literal.len() + column.name.len() * 2 + 8);
        if total_bytes > MAX_GENERATED_SQL_BYTES {
            return Err("The record exceeds the SQL preview size limit.".into());
        }
        included.push(identifier(engine, &column.name));
        literals.push(literal);
    }
    let statement = if included.is_empty() {
        if engine == Engine::MySql {
            format!("INSERT INTO {table} () VALUES ();\n")
        } else {
            format!("INSERT INTO {table} DEFAULT VALUES;\n")
        }
    } else {
        format!(
            "INSERT INTO {table} ({})\nVALUES ({});\n",
            included.join(", "),
            literals.join(", ")
        )
    };
    let mut sql = "-- Record insert preview. Review values before running.\n-- Omitted columns use database defaults; required values and constraints are checked on execution.\n".to_string();
    append_export(&mut sql, &statement, MAX_GENERATED_SQL_BYTES)?;
    Ok(sql)
}

/// Builds a DELETE guarded by every original column and non-NULL primary key.
/// Missing, computed, duplicate or unsupported columns fail closed.
pub fn delete_record_sql(
    engine: Engine,
    object: &DbObject,
    schema: &[ColumnDetails],
    result: &QueryResult,
    row_index: usize,
) -> Result<String, String> {
    let table = record_table(engine, object)?;
    let row = result
        .rows
        .get(row_index)
        .ok_or("Select a result row to delete.")?;
    if schema.is_empty()
        || schema.len() > 128
        || result.columns.len() != schema.len()
        || row.len() != schema.len()
    {
        return Err("Deleting requires the complete original table row.".into());
    }
    if !schema.iter().any(|column| column.primary_key) {
        return Err("Deleting a record safely requires a primary key.".into());
    }
    let mut names = HashSet::new();
    let mut predicates = Vec::new();
    let mut total_bytes = 0usize;
    for (index, column) in result.columns.iter().enumerate() {
        if !names.insert(&column.name) {
            return Err("Deleting requires unique original column names.".into());
        }
        let field = schema.iter().find(|field| field.name == column.name)
            .ok_or("Deleting requires the table's original columns, without computed or joined results.")?;
        validate_identifier(&field.name)?;
        if !supported_insert_type(&field.data_type) {
            return Err(
                "Binary, compound and unknown records require explicit SQL deletion.".into(),
            );
        }
        let name = identifier(engine, &field.name);
        let predicate = match row[index].as_deref() {
            None if field.primary_key => {
                return Err(
                    "Deleting requires non-NULL values for every primary-key column.".into(),
                );
            }
            None => format!("{name} IS NULL"),
            Some(value) => {
                if value.len() > MAX_GENERATED_SQL_BYTES {
                    return Err("A record value exceeds the SQL preview size limit.".into());
                }
                // Embedded SQLite permits blobs in scalar-declared columns.
                if matches!(engine, Engine::SQLite | Engine::MySql)
                    && value.strip_prefix("0x").is_some_and(|hex| {
                        hex.len().is_multiple_of(2)
                            && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                {
                    return Err("A hex-formatted value may represent binary data. Use explicit SQL deletion.".into());
                }
                format!("{name} = {}", text_literal(engine, value)?)
            }
        };
        total_bytes = total_bytes.saturating_add(predicate.len() + 8);
        if total_bytes > MAX_GENERATED_SQL_BYTES {
            return Err("The record exceeds the SQL preview size limit.".into());
        }
        predicates.push(predicate);
    }
    if schema.iter().any(|field| !names.contains(&field.name)) {
        return Err("Deleting requires every original table column.".into());
    }
    let mut sql = "-- Record deletion preview. Review before running: deletion can invoke triggers and cascading foreign keys.\n-- Expect one affected row; zero means the original record changed or was removed.\n".to_string();
    append_export(
        &mut sql,
        &format!(
            "DELETE FROM {table}\nWHERE {};\n",
            predicates.join("\n  AND ")
        ),
        MAX_GENERATED_SQL_BYTES,
    )?;
    Ok(sql)
}

fn record_table(engine: Engine, object: &DbObject) -> Result<String, String> {
    if object.kind != ObjectKind::Table {
        return Err("Record operations require a base table.".into());
    }
    validate_identifier(&object.name)?;
    if object.schema.is_empty() {
        Ok(identifier(engine, &object.name))
    } else {
        validate_identifier(&object.schema)?;
        Ok(format!(
            "{}.{}",
            identifier(engine, &object.schema),
            identifier(engine, &object.name)
        ))
    }
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > 128 || value.contains('\0') {
        return Err(
            "Table and column names must contain 1–128 characters and no NUL bytes.".into(),
        );
    }
    Ok(())
}

fn identifier(engine: Engine, value: &str) -> String {
    let quote = if engine == Engine::MySql { '`' } else { '"' };
    format!(
        "{quote}{}{quote}",
        value.replace(quote, &format!("{quote}{quote}"))
    )
}

fn text_literal(engine: Engine, value: &str) -> Result<String, String> {
    if value.contains('\0') {
        return Err("NUL bytes are not supported in text import or record edits.".into());
    }
    if engine == Engine::MySql {
        let mut hex = String::with_capacity(value.len() * 2);
        for byte in value.bytes() {
            write!(hex, "{byte:02x}").map_err(|_| "Cannot encode an imported value.")?;
        }
        // Hex avoids dependency on MySQL's NO_BACKSLASH_ESCAPES setting.
        Ok(format!("CONVERT(X'{hex}' USING utf8mb4)"))
    } else if engine == Engine::Postgres {
        // Explicit E strings remain predictable if standard_conforming_strings changes.
        Ok(format!(
            "E'{}'",
            value.replace('\\', "\\\\").replace('\'', "''")
        ))
    } else {
        Ok(format!("'{}'", value.replace('\'', "''")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{CancelToken, ConnectionConfig, DatabaseSession, ResultColumn};

    fn result() -> QueryResult {
        QueryResult {
            columns: vec![
                ResultColumn {
                    name: "text".into(),
                    data_type: "TEXT".into(),
                },
                ResultColumn {
                    name: "text".into(),
                    data_type: "TEXT".into(),
                },
            ],
            rows: vec![vec![Some("comma, quote\"\nand newline".into()), None]],
            affected_rows: 0,
            truncated: false,
        }
    }

    fn export_table() -> DbObject {
        DbObject {
            schema: String::new(),
            name: "items".into(),
            kind: ObjectKind::Table,
        }
    }

    #[test]
    fn record_inserts_preserve_defaults_nulls_and_literal_text() {
        for engine in [Engine::SQLite, Engine::DuckDB] {
            let session = DatabaseSession::connect(&ConnectionConfig {
                engine,
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            })
            .expect("scratch session");
            let cancel = CancelToken::new();
            session.execute("CREATE TABLE items(id INTEGER PRIMARY KEY DEFAULT 7, name TEXT DEFAULT 'default name', note TEXT); CREATE TABLE victim(id INTEGER);", 100, &cancel).expect("tables");
            let object = session
                .catalog()
                .expect("catalog")
                .into_iter()
                .find(|object| object.name == "items")
                .expect("table");
            let schema = session.describe(&object).expect("schema");
            let all_default = insert_record_sql(
                engine,
                &object,
                &schema,
                &vec![RecordValue::Default; schema.len()],
            )
            .expect("default draft");
            session
                .execute(&all_default, 100, &cancel)
                .expect("default insert");
            let value = "'); DROP TABLE victim;--\\next\nO'Brien";
            let sql = insert_record_sql(
                engine,
                &object,
                &schema,
                &[
                    RecordValue::Text("8".into()),
                    RecordValue::Text(value.into()),
                    RecordValue::Null,
                ],
            )
            .expect("literal insert");
            session.execute(&sql, 100, &cancel).expect("insert literal");
            let output = session
                .execute(
                    "SELECT name, note FROM items ORDER BY id; SELECT COUNT(*) FROM victim;",
                    100,
                    &cancel,
                )
                .expect("read records");
            assert_eq!(
                output.results[0].rows,
                vec![
                    vec![Some("default name".into()), None],
                    vec![Some(value.into()), None]
                ]
            );
            assert_eq!(output.results[1].rows[0][0].as_deref(), Some("0"));
            assert!(insert_record_sql(engine, &object, &schema, &[]).is_err());
        }
    }

    #[test]
    fn record_delete_checks_original_values_and_all_composite_keys() {
        for engine in [Engine::SQLite, Engine::DuckDB] {
            let session = DatabaseSession::connect(&ConnectionConfig {
                engine,
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            })
            .expect("scratch session");
            let cancel = CancelToken::new();
            session.execute("CREATE TABLE items(id INTEGER, part INTEGER, name TEXT, note TEXT, PRIMARY KEY(id, part)); INSERT INTO items VALUES(1, 1, 'old', NULL), (1, 2, 'old', NULL); CREATE TABLE victim(id INTEGER);", 100, &cancel).expect("tables");
            let object = session
                .catalog()
                .expect("catalog")
                .into_iter()
                .find(|object| object.name == "items")
                .expect("table");
            let schema = session.describe(&object).expect("schema");
            let original = session
                .execute("SELECT * FROM items ORDER BY part", 100, &cancel)
                .expect("original rows");
            let sql = delete_record_sql(engine, &object, &schema, &original.results[0], 0)
                .expect("delete draft");
            session
                .execute(
                    "UPDATE items SET name = 'changed' WHERE part = 1",
                    100,
                    &cancel,
                )
                .expect("concurrent edit");
            session.execute(&sql, 100, &cancel).expect("stale delete");
            assert_eq!(
                session
                    .execute("SELECT COUNT(*) FROM items", 100, &cancel)
                    .expect("remaining")
                    .results[0]
                    .rows[0][0]
                    .as_deref(),
                Some("2")
            );
            let injected = "'); DROP TABLE victim;--\\O'Brien";
            let update = edit_record_sql(
                engine,
                &object,
                &schema,
                &original.results[0],
                1,
                2,
                Some(injected),
            )
            .expect("safe edit");
            session
                .execute(&update, 100, &cancel)
                .expect("edit literal");
            let fresh = session
                .execute("SELECT * FROM items ORDER BY part", 100, &cancel)
                .expect("fresh rows");
            let sql = delete_record_sql(engine, &object, &schema, &fresh.results[0], 1)
                .expect("fresh deletion");
            session.execute(&sql, 100, &cancel).expect("delete one");
            let remaining = session
                .execute(
                    "SELECT part FROM items; SELECT COUNT(*) FROM victim",
                    100,
                    &cancel,
                )
                .expect("remaining tables");
            assert_eq!(remaining.results[0].rows, vec![vec![Some("1".into())]]);
            assert_eq!(remaining.results[1].rows[0][0].as_deref(), Some("0"));
            assert!(delete_record_sql(engine, &object, &[], &fresh.results[0], 0).is_err());
            let mut incomplete = fresh.results[0].clone();
            incomplete.columns.remove(1);
            for row in &mut incomplete.rows {
                row.remove(1);
            }
            assert!(delete_record_sql(engine, &object, &schema, &incomplete, 0).is_err());
            let mut null_key = fresh.results[0].clone();
            null_key.rows[0][0] = None;
            assert!(delete_record_sql(engine, &object, &schema, &null_key, 0).is_err());
        }
    }

    #[test]
    fn record_drafts_quote_targets_and_reject_unsupported_values() {
        let object = DbObject {
            schema: "s\"`".into(),
            name: "t\"`; DROP TABLE victim;--".into(),
            kind: ObjectKind::Table,
        };
        let schema = vec![ColumnDetails {
            name: "value\"`".into(),
            data_type: "TEXT".into(),
            nullable: true,
            primary_key: false,
        }];
        for engine in Engine::ALL {
            let sql = insert_record_sql(
                engine,
                &object,
                &schema,
                &[RecordValue::Text("'\\;--".into())],
            )
            .expect("quoted insert");
            assert!(sql.contains(&format!(
                "{}.{}",
                identifier(engine, &object.schema),
                identifier(engine, &object.name)
            )));
            let default_sql = insert_record_sql(engine, &object, &schema, &[RecordValue::Default])
                .expect("default insert");
            assert!(default_sql.contains(if engine == Engine::MySql {
                "() VALUES ()"
            } else {
                "DEFAULT VALUES"
            }));
            assert!(
                insert_record_sql(
                    engine,
                    &object,
                    &schema,
                    &[RecordValue::Text("a\0b".into())]
                )
                .is_err()
            );
        }
        let mut binary = schema.clone();
        binary[0].data_type = "BLOB".into();
        assert!(
            insert_record_sql(
                Engine::SQLite,
                &object,
                &binary,
                &[RecordValue::Text("blob".into())]
            )
            .is_err()
        );
        assert!(
            insert_record_sql(Engine::SQLite, &object, &binary, &[RecordValue::Default]).is_ok()
        );
        let mut required = schema.clone();
        required[0].nullable = false;
        assert!(
            insert_record_sql(Engine::SQLite, &object, &required, &[RecordValue::Null]).is_err()
        );
        let original = QueryResult {
            columns: vec![ResultColumn {
                name: schema[0].name.clone(),
                data_type: "TEXT".into(),
            }],
            rows: vec![vec![Some("value".into())]],
            ..Default::default()
        };
        assert!(
            delete_record_sql(Engine::SQLite, &object, &schema, &original, 0)
                .expect_err("missing primary key")
                .contains("primary key")
        );
        binary[0].primary_key = true;
        assert!(
            delete_record_sql(Engine::SQLite, &object, &binary, &original, 0)
                .expect_err("binary record")
                .contains("Binary")
        );
        let view = DbObject {
            kind: ObjectKind::View,
            ..object.clone()
        };
        assert!(
            insert_record_sql(Engine::SQLite, &view, &schema, &[RecordValue::Default]).is_err()
        );
        assert!(delete_record_sql(Engine::SQLite, &view, &schema, &original, 0).is_err());
    }

    #[test]
    fn markdown_keeps_duplicate_headers_and_escapes_markup_and_nulls() {
        let mut output = result();
        output.columns[0].name = "x|y".into();
        output.columns[1].name = "x|y".into();
        output.rows[0][0] = Some("<script>&[link](url)*_`\\|\r\nnext".into());
        assert_eq!(
            result_to_markdown(&output).expect("Markdown export"),
            "| x\\|y | x\\|y |\n| --- | --- |\n| &lt;script&gt;&amp;\\[link\\](url)\\*\\_\\`\\\\\\|<br>next | *NULL* |\n"
        );
    }

    #[test]
    fn insert_quotes_schema_table_columns_and_values_for_every_engine() {
        let mut output = result();
        output.columns[0].name = "a\"`".into();
        output.columns[1].name = "nullable".into();
        output.rows[0][0] = Some("O'Brien\\'; DROP TABLE victim;--".into());
        let table = DbObject {
            schema: "schema\"`".into(),
            name: "table\"`; DROP TABLE victim;--".into(),
            kind: ObjectKind::Table,
        };
        for engine in Engine::ALL {
            let sql = result_to_insert_sql(engine, &table, &output).expect("safe INSERT");
            let qualified = format!(
                "{}.{}",
                identifier(engine, &table.schema),
                identifier(engine, &table.name)
            );
            assert!(sql.contains(&format!(
                "INSERT INTO {qualified} ({}, {}",
                identifier(engine, &output.columns[0].name),
                identifier(engine, "nullable")
            )));
            assert!(sql.contains(&format!(
                "{}, NULL);",
                text_literal(engine, output.rows[0][0].as_deref().expect("text")).expect("literal")
            )));
        }
    }

    #[test]
    fn insert_round_trips_text_and_null_without_executing_injected_sql() {
        for engine in [Engine::SQLite, Engine::DuckDB] {
            let session = DatabaseSession::connect(&ConnectionConfig {
                engine,
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            })
            .expect("scratch database");
            let cancel = CancelToken::new();
            session.execute("CREATE TABLE items(name TEXT, nullable TEXT); CREATE TABLE victim(id INTEGER);", 100, &cancel).expect("scratch tables");
            let mut output = result();
            output.columns[0].name = "name".into();
            output.columns[1].name = "nullable".into();
            output.rows[0][0] = Some("'); DROP TABLE victim;--\\next\nO'Brien".into());
            let sql =
                result_to_insert_sql(engine, &export_table(), &output).expect("INSERT preview");
            session
                .execute(&sql, 100, &cancel)
                .expect("execute reviewed preview");
            let stored = session
                .execute(
                    "SELECT name, nullable FROM items; SELECT COUNT(*) FROM victim;",
                    100,
                    &cancel,
                )
                .expect("table remains");
            assert_eq!(stored.results[0].rows, output.rows);
            assert_eq!(stored.results[1].rows[0][0].as_deref(), Some("0"));
        }
    }

    #[test]
    fn insert_rejects_ambiguous_headers_unknown_types_and_binary_values() {
        let mut output = result();
        assert!(result_to_insert_sql(Engine::SQLite, &export_table(), &output).is_err());
        output.columns[1].name = "other".into();
        for unsupported in [
            "BLOB",
            "bytea",
            "Int64[]",
            "List(Int64)",
            "Struct(a Int64)",
            "MYSQL_TYPE_BLOB",
            "expression",
            "custom_type",
        ] {
            output.columns[0].data_type = unsupported.into();
            assert!(
                result_to_insert_sql(Engine::SQLite, &export_table(), &output).is_err(),
                "unsupported type: {unsupported}"
            );
        }
        output.columns[0].data_type = "TEXT".into();
        output.rows[0][0] = Some("0xDEADBEEF".into());
        assert!(result_to_insert_sql(Engine::SQLite, &export_table(), &output).is_err());
        output.rows[0][0] = Some("a\0b".into());
        assert!(result_to_insert_sql(Engine::SQLite, &export_table(), &output).is_err());
    }

    #[test]
    fn new_exports_reject_malformed_rows_and_enforce_output_limits() {
        let mut output = result();
        output.columns[1].name = "other".into();
        output.rows[0].pop();
        assert!(result_to_markdown(&output).is_err());
        assert!(result_to_insert_sql(Engine::SQLite, &export_table(), &output).is_err());
        output.rows[0].push(None);
        output.rows[0][0] = Some("&".repeat(MAX_MARKDOWN_BYTES / 5 + 1));
        assert!(result_to_markdown(&output).is_err());
        output.rows[0][0] = Some("'".repeat(MAX_GENERATED_SQL_BYTES / 2 + 1));
        assert!(result_to_insert_sql(Engine::SQLite, &export_table(), &output).is_err());
    }

    #[test]
    fn insert_supports_driver_scalar_metadata_and_marks_truncated_results() {
        for supported in [
            "varchar(255)",
            "Int64",
            "UInt64",
            "Decimal128(20, 2)",
            "Timestamp(Microsecond, None)",
            "MYSQL_TYPE_NEWDECIMAL",
            "MYSQL_TYPE_VAR_STRING",
        ] {
            assert!(
                supported_insert_type(supported),
                "supported type: {supported}"
            );
        }
        let mut output = result();
        output.columns[1].name = "other".into();
        output.truncated = true;
        assert!(
            result_to_insert_sql(Engine::SQLite, &export_table(), &output)
                .expect("INSERT")
                .contains("only loaded rows are included")
        );
    }

    #[test]
    fn exports_preserve_csv_quoting_and_json_nulls_and_duplicate_names() {
        let output = result();
        let csv = result_to_csv(&output).expect("export CSV");
        let mut reader = csv::Reader::from_reader(csv.as_bytes());
        let row = reader.records().next().expect("record").expect("valid CSV");
        assert_eq!(&row[0], output.rows[0][0].as_deref().expect("text"));
        let json: serde_json::Value =
            serde_json::from_str(&result_to_json(&output).expect("export JSON"))
                .expect("valid JSON");
        assert_eq!(json["columns"].as_array().expect("columns").len(), 2);
        assert!(json["rows"][0][1].is_null());
    }

    #[test]
    fn csv_import_runs_locally_without_interpreting_names_or_values_as_sql() {
        for engine in [Engine::SQLite, Engine::DuckDB] {
            let config = ConnectionConfig {
                engine,
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            };
            let session = DatabaseSession::connect(&config).expect("scratch database");
            let sql = import_csv_sql(
                engine,
                "odd\";name",
                "name,description\nO'Brien,\"comma, and newline\nnext\"\n",
            )
            .expect("import preview");
            session
                .execute(&sql, 100, &CancelToken::new())
                .expect("run import");
            let output = session
                .execute(
                    "SELECT name, description FROM \"odd\"\";name\"",
                    100,
                    &CancelToken::new(),
                )
                .expect("read imported data");
            let result = output.results.last().expect("result");
            assert_eq!(result.rows[0][0].as_deref(), Some("O'Brien"));
            assert_eq!(
                result.rows[0][1].as_deref(),
                Some("comma, and newline\nnext")
            );
        }
    }

    #[test]
    fn import_rejects_duplicate_headers_and_binary_text() {
        assert!(import_csv_sql(Engine::SQLite, "items", "Name,name\na,b\n").is_err());
        assert!(import_csv_sql(Engine::SQLite, "items", "name\na\0b\n").is_err());
        assert!(import_csv_sql(Engine::SQLite, "", "name\na\n").is_err());
    }

    #[test]
    fn nullable_primary_keys_cannot_generate_multirow_edits() {
        let session = DatabaseSession::connect(&ConnectionConfig {
            engine: Engine::SQLite,
            database: ":memory:".into(),
            read_only: false,
            ..Default::default()
        })
        .expect("scratch database");
        let cancel = CancelToken::new();
        session.execute("CREATE TABLE items(id TEXT PRIMARY KEY, name TEXT); INSERT INTO items VALUES(NULL, 'old'), (NULL, 'old');", 100, &cancel).expect("nullable keys");
        let object = session
            .catalog()
            .expect("catalog")
            .into_iter()
            .find(|object| object.name == "items")
            .expect("table");
        let schema = session.describe(&object).expect("schema");
        let output = session
            .execute("SELECT id, name FROM items", 100, &cancel)
            .expect("rows");
        let result = output.results.last().expect("result");
        assert_eq!(result.rows.len(), 2);
        assert!(
            edit_record_sql(Engine::SQLite, &object, &schema, result, 0, 1, Some("new")).is_err()
        );
    }

    #[test]
    fn record_edits_require_keys_and_do_not_overwrite_changed_rows() {
        let session = DatabaseSession::connect(&ConnectionConfig {
            engine: Engine::SQLite,
            database: ":memory:".into(),
            read_only: false,
            ..Default::default()
        })
        .expect("scratch database");
        let cancel = CancelToken::new();
        session.execute("CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES(1, 'old');", 100, &cancel).expect("seed scratch");
        let object = session
            .catalog()
            .expect("catalog")
            .into_iter()
            .find(|object| object.name == "items")
            .expect("table");
        let schema = session.describe(&object).expect("schema");
        let output = session
            .execute("SELECT id, name FROM items;", 100, &cancel)
            .expect("table data");
        let result = output.results.last().expect("result");
        let edit = edit_record_sql(
            Engine::SQLite,
            &object,
            &schema,
            result,
            0,
            1,
            Some("new's value"),
        )
        .expect("safe edit");
        session
            .execute(
                "UPDATE items SET name = 'changed elsewhere' WHERE id = 1;",
                100,
                &cancel,
            )
            .expect("concurrent change");
        let affected = session
            .execute(&edit, 100, &cancel)
            .expect("conditional edit");
        assert_eq!(
            affected
                .results
                .last()
                .expect("update result")
                .affected_rows,
            0
        );
        assert!(edit_record_sql(Engine::SQLite, &object, &[], result, 0, 1, None).is_err());
        let fresh = session
            .execute("SELECT id, name FROM items;", 100, &cancel)
            .expect("fresh table data");
        let edit = edit_record_sql(
            Engine::SQLite,
            &object,
            &schema,
            fresh.results.last().expect("fresh result"),
            0,
            1,
            Some("new's value"),
        )
        .expect("fresh edit");
        let affected = session
            .execute(&edit, 100, &cancel)
            .expect("apply fresh edit");
        assert_eq!(
            affected
                .results
                .last()
                .expect("updated result")
                .affected_rows,
            1
        );
    }
}
