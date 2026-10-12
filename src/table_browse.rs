//! Typed, reviewable table browsing queries. No user-provided SQL fragments are accepted.

use std::fmt::Write as _;

use crate::database::{ColumnDetails, DbObject, Engine, MAX_ROWS};

const MAX_FILTER_BYTES: usize = 64 * 1024;

/// A bounded table or view query; `page` is zero based.
#[derive(Clone, Debug)]
pub struct BrowseRequest {
    pub object: DbObject,
    pub filter: Option<BrowseFilter>,
    pub sort: Option<BrowseSort>,
    pub limit: usize,
    pub page: usize,
}

/// A single column predicate. NULL predicates ignore `value`.
#[derive(Clone, Debug)]
pub struct BrowseFilter {
    pub column: String,
    pub operator: FilterOperator,
    pub value: String,
}

/// Supported predicates; `Contains` searches for a literal substring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    IsNull,
    IsNotNull,
}

/// A column sort, using the database's native ordering and collation.
#[derive(Clone, Debug)]
pub struct BrowseSort {
    pub column: String,
    pub descending: bool,
}

/// Builds one SELECT using catalog column names and safely encoded literals.
/// Primary keys provide a default ordering and break ties in explicit sorts.
/// Offset pages do not provide a snapshot across concurrent database changes.
///
/// # Errors
/// Rejects invalid names, unknown columns, oversized values, invalid page sizes,
/// and offsets outside the signed 64-bit range accepted by the supported engines.
pub fn browse_sql(
    engine: Engine,
    request: &BrowseRequest,
    columns: &[ColumnDetails],
) -> Result<String, String> {
    validate_identifier(&request.object.name)?;
    if !request.object.schema.is_empty() {
        validate_identifier(&request.object.schema)?;
    }
    if !(1..=MAX_ROWS).contains(&request.limit) {
        return Err("Page size must be between 1 and 10,000 rows.".into());
    }
    let offset = request
        .page
        .checked_mul(request.limit)
        .and_then(|offset| i64::try_from(offset).ok())
        .ok_or("The page offset exceeds the supported range.")?;
    let table = if request.object.schema.is_empty() {
        engine.quote_identifier(&request.object.name)
    } else {
        format!(
            "{}.{}",
            engine.quote_identifier(&request.object.schema),
            engine.quote_identifier(&request.object.name)
        )
    };
    let mut sql = format!("SELECT * FROM {table}");
    if let Some(filter) = &request.filter {
        validate_column(&filter.column, columns)?;
        let predicate = filter_predicate(engine, filter)?;
        sql.push_str(&format!("\nWHERE {predicate}"));
    }
    let mut ordering = Vec::new();
    if let Some(sort) = &request.sort {
        validate_column(&sort.column, columns)?;
        ordering.push(format!(
            "{} {}",
            engine.quote_identifier(&sort.column),
            if sort.descending { "DESC" } else { "ASC" }
        ));
    }
    for column in columns.iter().filter(|column| column.primary_key) {
        if request
            .sort
            .as_ref()
            .is_some_and(|sort| sort.column == column.name)
        {
            continue;
        }
        validate_identifier(&column.name)?;
        ordering.push(format!("{} ASC", engine.quote_identifier(&column.name)));
    }
    if !ordering.is_empty() {
        sql.push_str(&format!("\nORDER BY {}", ordering.join(", ")));
    }
    sql.push_str(&format!("\nLIMIT {} OFFSET {offset};", request.limit));
    Ok(sql)
}

fn validate_identifier(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('\0') {
        return Err(
            "Table, schema and column names must be nonempty and contain no NUL bytes.".into(),
        );
    }
    Ok(())
}

fn validate_column(name: &str, columns: &[ColumnDetails]) -> Result<(), String> {
    validate_identifier(name)?;
    if !columns.iter().any(|column| column.name == name) {
        return Err("Choose a column from the selected table's schema.".into());
    }
    Ok(())
}

fn filter_predicate(engine: Engine, filter: &BrowseFilter) -> Result<String, String> {
    let column = engine.quote_identifier(&filter.column);
    match filter.operator {
        FilterOperator::IsNull => Ok(format!("{column} IS NULL")),
        FilterOperator::IsNotNull => Ok(format!("{column} IS NOT NULL")),
        FilterOperator::Equals => Ok(format!(
            "{column} = {}",
            text_literal(engine, &filter.value)?
        )),
        FilterOperator::NotEquals => Ok(format!(
            "{column} <> {}",
            text_literal(engine, &filter.value)?
        )),
        FilterOperator::Contains => {
            let value = text_literal(engine, &filter.value)?;
            // Substring functions preserve literal percent/underscore characters
            // and avoid LIKE escape differences between connection settings.
            Ok(match engine {
                Engine::SQLite | Engine::DuckDB => {
                    format!("INSTR(CAST({column} AS TEXT), {value}) > 0")
                }
                Engine::Postgres => format!("STRPOS(CAST({column} AS TEXT), {value}) > 0"),
                Engine::MySql => {
                    format!("LOCATE({value}, CAST({column} AS CHAR CHARACTER SET utf8mb4)) > 0")
                }
            })
        }
    }
}

fn text_literal(engine: Engine, value: &str) -> Result<String, String> {
    if value.len() > MAX_FILTER_BYTES || value.contains('\0') {
        return Err("Filter values must contain no NUL bytes and fit within 64 KiB.".into());
    }
    match engine {
        Engine::MySql => {
            let mut hex = String::with_capacity(value.len() * 2);
            for byte in value.bytes() {
                write!(hex, "{byte:02x}").map_err(|_| "The filter value could not be encoded.")?;
            }
            // Hex is independent of MySQL's NO_BACKSLASH_ESCAPES setting.
            Ok(format!("CONVERT(X'{hex}' USING utf8mb4)"))
        }
        Engine::Postgres => {
            // Explicit E strings are independent of standard_conforming_strings.
            Ok(format!(
                "E'{}'",
                value.replace('\\', "\\\\").replace('\'', "''")
            ))
        }
        Engine::SQLite | Engine::DuckDB => Ok(format!("'{}'", value.replace('\'', "''"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{CancelToken, ConnectionConfig, DatabaseSession, ObjectKind};

    fn columns() -> Vec<ColumnDetails> {
        vec![
            ColumnDetails {
                name: "id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                primary_key: true,
            },
            ColumnDetails {
                name: "name".into(),
                data_type: "TEXT".into(),
                nullable: true,
                primary_key: false,
            },
        ]
    }

    fn request() -> BrowseRequest {
        BrowseRequest {
            object: DbObject {
                schema: String::new(),
                name: "items".into(),
                kind: ObjectKind::Table,
            },
            filter: None,
            sort: None,
            limit: 2,
            page: 0,
        }
    }

    #[test]
    fn page_sizes_and_overflowing_offsets_are_rejected() {
        let mut request = request();
        for limit in [0, MAX_ROWS + 1, usize::MAX] {
            request.limit = limit;
            assert!(browse_sql(Engine::SQLite, &request, &columns()).is_err());
        }
        request.limit = MAX_ROWS;
        request.page = usize::MAX;
        assert!(browse_sql(Engine::SQLite, &request, &columns()).is_err());
    }

    #[test]
    fn unknown_columns_cannot_be_used_as_sql_fragments() {
        let mut request = request();
        request.sort = Some(BrowseSort {
            column: "name; DROP TABLE items".into(),
            descending: false,
        });
        assert!(browse_sql(Engine::SQLite, &request, &columns()).is_err());
        request.sort = None;
        request.filter = Some(BrowseFilter {
            column: "missing".into(),
            operator: FilterOperator::Equals,
            value: "value".into(),
        });
        assert!(browse_sql(Engine::SQLite, &request, &columns()).is_err());
    }

    #[test]
    fn null_predicates_ignore_the_text_value() {
        let mut request = request();
        for (operator, expected) in [
            (FilterOperator::IsNull, "IS NULL"),
            (FilterOperator::IsNotNull, "IS NOT NULL"),
        ] {
            request.filter = Some(BrowseFilter {
                column: "name".into(),
                operator,
                value: "ignored\0".into(),
            });
            let sql = browse_sql(Engine::Postgres, &request, &columns()).expect("NULL query");
            assert!(sql.contains(&format!("WHERE \"name\" {expected}")));
        }
    }

    #[test]
    fn dialects_encode_names_and_values_independently_of_sql_modes() {
        let mut request = request();
        request.object.schema = "schema\"`".into();
        request.object.name = "table\"`".into();
        request.filter = Some(BrowseFilter {
            column: "name".into(),
            operator: FilterOperator::Equals,
            value: "\\'; SELECT 1; --".into(),
        });
        let mysql = browse_sql(Engine::MySql, &request, &columns()).expect("MySQL query");
        assert!(mysql.contains("FROM `schema\"```.`table\"```"));
        assert!(mysql.contains("CONVERT(X'5c273b2053454c45435420313b202d2d' USING utf8mb4)"));
        let postgres =
            browse_sql(Engine::Postgres, &request, &columns()).expect("PostgreSQL query");
        assert!(postgres.contains("E'\\\\''; SELECT 1; --'"));
    }

    #[test]
    fn text_values_reject_nul_and_unbounded_input() {
        for engine in Engine::ALL {
            assert!(text_literal(engine, "nul\0").is_err());
            assert!(text_literal(engine, &"x".repeat(MAX_FILTER_BYTES + 1)).is_err());
        }
    }

    #[test]
    fn embedded_queries_filter_literal_substrings_sort_and_page_without_mutating() {
        for engine in [Engine::SQLite, Engine::DuckDB] {
            let session = DatabaseSession::connect(&ConnectionConfig {
                engine,
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            })
            .expect("scratch database");
            let cancel = CancelToken::new();
            session.execute("CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES(1, '50%_off'), (2, 'other'), (3, NULL), (4, '50%_off');", 100, &cancel).expect("seed database");
            let mut request = request();
            request.filter = Some(BrowseFilter {
                column: "name".into(),
                operator: FilterOperator::Contains,
                value: "%_".into(),
            });
            request.sort = Some(BrowseSort {
                column: "id".into(),
                descending: true,
            });
            request.limit = 1;
            request.page = 1;
            let sql = browse_sql(engine, &request, &columns()).expect("browse query");
            let output = session.execute(&sql, 100, &cancel).expect("browse");
            assert_eq!(
                output.results[0].rows,
                vec![vec![Some("1".into()), Some("50%_off".into())]]
            );
            request.page = 0;
            request.filter.as_mut().expect("filter").operator = FilterOperator::IsNull;
            let sql = browse_sql(engine, &request, &columns()).expect("NULL query");
            let output = session.execute(&sql, 100, &cancel).expect("NULL browse");
            assert_eq!(output.results[0].rows[0][0].as_deref(), Some("3"));
            request.filter = Some(BrowseFilter {
                column: "name".into(),
                operator: FilterOperator::NotEquals,
                value: "'; DROP TABLE items; --".into(),
            });
            request.limit = 100;
            let sql = browse_sql(engine, &request, &columns()).expect("injection value query");
            let output = session
                .execute(&sql, 100, &cancel)
                .expect("safe comparison");
            assert_eq!(output.results[0].rows.len(), 3);
            request.filter = Some(BrowseFilter {
                column: "id".into(),
                operator: FilterOperator::Equals,
                value: "2".into(),
            });
            let sql = browse_sql(engine, &request, &columns()).expect("numeric query");
            let output = session
                .execute(&sql, 100, &cancel)
                .expect("numeric comparison");
            assert_eq!(output.results[0].rows.len(), 1);
        }
    }
}
