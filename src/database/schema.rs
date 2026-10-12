//! Read-only schema inspection uses the same bounded driver path as query execution.

use super::{
    CancelToken, ColumnDetails, DatabaseSession, DbError, DbObject, Engine, MAX_RESULT_BYTES,
    MAX_ROWS, QueryResult, cell,
};
use std::collections::BTreeMap;

/// A complete inspection result for a table or view.
#[derive(Clone, Debug, Default)]
pub struct TableDetails {
    pub columns: Vec<ColumnDetails>,
    pub indexes: Vec<IndexDetails>,
    pub foreign_keys: Vec<ForeignKeyDetails>,
}

/// Index keys are kept in database order; expressions are displayed as expressions.
#[derive(Clone, Debug)]
pub struct IndexDetails {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary_key: bool,
    pub definition: Option<String>,
}

/// Source and referenced columns have matching positions for composite keys.
#[derive(Clone, Debug)]
pub struct ForeignKeyDetails {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

impl DatabaseSession {
    /// Inspect columns, indexes and outbound foreign keys without changing the session.
    /// Call from a background job, as with `describe` and `catalog`.
    pub fn table_details(&self, object: &DbObject) -> Result<super::TableDetails, DbError> {
        if object.name.contains('\0') || object.schema.contains('\0') {
            return Err(DbError(
                "Database object names cannot contain NUL characters.".into(),
            ));
        }
        let columns = self.describe(object)?;
        let (indexes, foreign_keys) = match self.engine {
            Engine::SQLite => self.sqlite_details(object, &columns)?,
            _ => (
                self.server_indexes(object)?,
                self.server_foreign_keys(object)?,
            ),
        };
        let details = TableDetails {
            columns,
            indexes,
            foreign_keys,
        };
        if details_size(&details) > MAX_RESULT_BYTES {
            return Err(DbError(
                "Schema metadata exceeds the 16 MiB display budget.".into(),
            ));
        }
        Ok(details)
    }

    fn metadata(&self, sql: &str) -> Result<QueryResult, DbError> {
        let output = self.execute(sql, MAX_ROWS, &CancelToken::new())?;
        let result = output
            .results
            .into_iter()
            .next()
            .ok_or_else(|| DbError("Schema inspection returned no result.".into()))?;
        if result.truncated {
            return Err(DbError("Schema metadata exceeds the display limit.".into()));
        }
        Ok(result)
    }

    fn sqlite_details(
        &self,
        object: &DbObject,
        columns: &[ColumnDetails],
    ) -> Result<(Vec<IndexDetails>, Vec<ForeignKeyDetails>), DbError> {
        let schema = Engine::SQLite.quote_identifier(if object.schema.is_empty() {
            "main"
        } else {
            &object.schema
        });
        let table = Engine::SQLite.quote_identifier(&object.name);
        let rows = self.metadata(&format!("PRAGMA {schema}.index_list({table})"))?;
        let mut indexes = Vec::new();
        let mut key_count = 0;
        let mut index_bytes = 0_usize;
        for row in rows.rows {
            let name = cell(&row, 1);
            let keys = self.metadata(&format!(
                "PRAGMA {schema}.index_info({})",
                Engine::SQLite.quote_identifier(&name)
            ))?;
            key_count += keys.rows.len();
            if key_count > MAX_ROWS {
                return Err(DbError("Index metadata exceeds the display limit.".into()));
            }
            let definition = self
                .metadata(&format!(
                    "SELECT sql FROM {schema}.sqlite_schema WHERE type = 'index' AND name = {}",
                    literal(Engine::SQLite, &name)
                ))?
                .rows
                .first()
                .and_then(|row| row.first())
                .cloned()
                .flatten();
            index_bytes = index_bytes.saturating_add(
                std::mem::size_of::<IndexDetails>()
                    + name.len()
                    + definition.as_ref().map_or(0, String::len)
                    + keys
                        .rows
                        .iter()
                        .map(|row| {
                            std::mem::size_of::<String>()
                                + row.get(2).and_then(Option::as_ref).map_or(12, String::len)
                        })
                        .sum::<usize>(),
            );
            if index_bytes > MAX_RESULT_BYTES {
                return Err(DbError(
                    "Index metadata exceeds the 16 MiB display budget.".into(),
                ));
            }
            indexes.push(IndexDetails {
                name,
                columns: keys
                    .rows
                    .iter()
                    .map(|row| {
                        row.get(2)
                            .and_then(Option::as_ref)
                            .cloned()
                            .unwrap_or_else(|| "(expression)".into())
                    })
                    .collect(),
                unique: cell(&row, 2) == "1",
                primary_key: cell(&row, 3) == "pk",
                definition,
            });
        }
        // An INTEGER PRIMARY KEY aliases the SQLite rowid and has no index_list row.
        if !indexes.iter().any(|index| index.primary_key) {
            let primary: Vec<_> = columns
                .iter()
                .filter(|column| column.primary_key)
                .map(|column| column.name.clone())
                .collect();
            if !primary.is_empty() {
                indexes.push(IndexDetails {
                    name: "PRIMARY KEY (rowid)".into(),
                    columns: primary,
                    unique: true,
                    primary_key: true,
                    definition: None,
                });
            }
        }
        indexes.sort_by(|left, right| left.name.cmp(&right.name));
        let rows = self.metadata(&format!("PRAGMA {schema}.foreign_key_list({table})"))?;
        let mut foreign_keys = BTreeMap::<String, ForeignKeyDetails>::new();
        // Sort numerically: lexical ordering would put composite-key position 10 before 2.
        let mut rows = rows.rows;
        rows.sort_by_key(|row| {
            (
                cell(row, 0).parse::<usize>().unwrap_or_default(),
                cell(row, 1).parse::<usize>().unwrap_or_default(),
            )
        });
        for row in rows {
            let name = format!("fk_{}", cell(&row, 0));
            let key = foreign_keys
                .entry(name.clone())
                .or_insert_with(|| ForeignKeyDetails {
                    name,
                    columns: Vec::new(),
                    referenced_schema: if object.schema.is_empty() {
                        "main".into()
                    } else {
                        object.schema.clone()
                    },
                    referenced_table: cell(&row, 2),
                    referenced_columns: Vec::new(),
                    on_update: cell(&row, 5),
                    on_delete: cell(&row, 6),
                });
            key.columns.push(cell(&row, 3));
            // SQLite uses NULL when REFERENCES omits the parent primary-key columns.
            key.referenced_columns.push(
                row.get(4)
                    .and_then(Option::as_ref)
                    .cloned()
                    .unwrap_or_else(|| "(primary key)".into()),
            );
        }
        Ok((indexes, foreign_keys.into_values().collect()))
    }

    fn server_indexes(&self, object: &DbObject) -> Result<Vec<IndexDetails>, DbError> {
        let schema = literal(self.engine, &object.schema);
        let table = literal(self.engine, &object.name);
        let sql = match self.engine {
            Engine::DuckDB => format!(
                "SELECT index_name, expressions, CASE WHEN is_unique THEN 'YES' ELSE 'NO' END, CASE WHEN is_primary THEN 'YES' ELSE 'NO' END, sql FROM duckdb_indexes() WHERE database_name = current_database() AND schema_name = {schema} AND table_name = {table} ORDER BY index_name"
            ),
            Engine::Postgres => format!(
                "SELECT i.relname, pg_get_indexdef(x.indexrelid, k.ordinality::int, true), CASE WHEN x.indisunique THEN 'YES' ELSE 'NO' END, CASE WHEN x.indisprimary THEN 'YES' ELSE 'NO' END, pg_get_indexdef(x.indexrelid) FROM pg_catalog.pg_index x JOIN pg_catalog.pg_class t ON t.oid = x.indrelid JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace JOIN pg_catalog.pg_class i ON i.oid = x.indexrelid CROSS JOIN LATERAL unnest(x.indkey) WITH ORDINALITY k(attnum, ordinality) WHERE n.nspname = {schema} AND t.relname = {table} ORDER BY i.relname, k.ordinality"
            ),
            Engine::MySql => format!(
                "SELECT INDEX_NAME, COALESCE(COLUMN_NAME, '(expression)'), CASE WHEN NON_UNIQUE = 0 THEN 'YES' ELSE 'NO' END, CASE WHEN INDEX_NAME = 'PRIMARY' THEN 'YES' ELSE 'NO' END, NULL FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {table} ORDER BY INDEX_NAME, SEQ_IN_INDEX"
            ),
            Engine::SQLite => return Err(DbError("SQLite uses PRAGMA index inspection.".into())),
        };
        let mut indexes = BTreeMap::<String, IndexDetails>::new();
        for row in self.metadata(&sql)?.rows {
            append_index(&mut indexes, &row);
        }
        if self.engine == Engine::DuckDB {
            // DuckDB's expressions field is reserved and may be NULL. Parse its
            // generated DDL with the already pinned SQL parser instead of splitting commas.
            for index in indexes.values_mut() {
                if let Some(definition) = &index.definition
                    && let Ok(statements) = sqlparser::parser::Parser::parse_sql(
                        &sqlparser::dialect::DuckDbDialect {},
                        definition,
                    )
                    && let Some(sqlparser::ast::Statement::CreateIndex(create)) = statements.first()
                {
                    index.columns = create
                        .columns
                        .iter()
                        .map(|column| column.column.to_string())
                        .collect();
                }
                if index.columns.iter().all(String::is_empty) {
                    index.columns = vec!["(see index definition)".into()];
                }
            }
            // DuckDB exposes indexes backing PRIMARY KEY/UNIQUE only as constraints.
            let sql = format!(
                "SELECT constraint_name, unnest(constraint_column_names), 'YES', CASE WHEN constraint_type = 'PRIMARY KEY' THEN 'YES' ELSE 'NO' END, constraint_text FROM duckdb_constraints() WHERE database_name = current_database() AND schema_name = {schema} AND table_name = {table} AND constraint_type IN ('PRIMARY KEY', 'UNIQUE') ORDER BY constraint_index"
            );
            let rows = self.metadata(&sql)?;
            if rows.rows.len()
                + indexes
                    .values()
                    .map(|index| index.columns.len())
                    .sum::<usize>()
                > MAX_ROWS
            {
                return Err(DbError("Index metadata exceeds the display limit.".into()));
            }
            for row in rows.rows {
                append_index(&mut indexes, &row);
            }
        }
        Ok(indexes.into_values().collect())
    }

    fn server_foreign_keys(&self, object: &DbObject) -> Result<Vec<ForeignKeyDetails>, DbError> {
        let schema = literal(self.engine, &object.schema);
        let table = literal(self.engine, &object.name);
        let sql = match self.engine {
            Engine::DuckDB => format!(
                "SELECT constraint_name, unnest(constraint_column_names), schema_name, referenced_table, unnest(referenced_column_names), 'NO ACTION', 'NO ACTION' FROM duckdb_constraints() WHERE database_name = current_database() AND schema_name = {schema} AND table_name = {table} AND constraint_type = 'FOREIGN KEY' ORDER BY constraint_index"
            ),
            Engine::Postgres => format!(
                "SELECT c.conname, a.attname, rn.nspname, rt.relname, ra.attname, c.confupdtype::text, c.confdeltype::text FROM pg_catalog.pg_constraint c JOIN pg_catalog.pg_class t ON t.oid = c.conrelid JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace JOIN pg_catalog.pg_class rt ON rt.oid = c.confrelid JOIN pg_catalog.pg_namespace rn ON rn.oid = rt.relnamespace CROSS JOIN LATERAL generate_subscripts(c.conkey, 1) AS k(pos) JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = c.conkey[k.pos] JOIN pg_catalog.pg_attribute ra ON ra.attrelid = rt.oid AND ra.attnum = c.confkey[k.pos] WHERE c.contype = 'f' AND n.nspname = {schema} AND t.relname = {table} ORDER BY c.conname, k.pos"
            ),
            Engine::MySql => format!(
                "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME AND r.TABLE_NAME = k.TABLE_NAME WHERE k.TABLE_SCHEMA = {schema} AND k.TABLE_NAME = {table} AND k.REFERENCED_TABLE_NAME IS NOT NULL ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION"
            ),
            Engine::SQLite => {
                return Err(DbError("SQLite uses PRAGMA foreign-key inspection.".into()));
            }
        };
        let mut keys = BTreeMap::<String, ForeignKeyDetails>::new();
        for row in self.metadata(&sql)?.rows {
            let name = cell(&row, 0);
            let key = keys
                .entry(name.clone())
                .or_insert_with(|| ForeignKeyDetails {
                    name,
                    columns: Vec::new(),
                    referenced_schema: cell(&row, 2),
                    referenced_table: cell(&row, 3),
                    referenced_columns: Vec::new(),
                    on_update: action(&cell(&row, 5)),
                    on_delete: action(&cell(&row, 6)),
                });
            key.columns.push(cell(&row, 1));
            key.referenced_columns.push(cell(&row, 4));
        }
        Ok(keys.into_values().collect())
    }
}

fn append_index(indexes: &mut BTreeMap<String, IndexDetails>, row: &[Option<String>]) {
    let name = cell(row, 0);
    let index = indexes.entry(name.clone()).or_insert_with(|| IndexDetails {
        name,
        columns: Vec::new(),
        unique: cell(row, 2) == "YES",
        primary_key: cell(row, 3) == "YES",
        definition: row.get(4).cloned().flatten(),
    });
    index.columns.push(cell(row, 1));
}

fn details_size(details: &TableDetails) -> usize {
    let columns = details
        .columns
        .iter()
        .map(|column| {
            std::mem::size_of::<ColumnDetails>() + column.name.len() + column.data_type.len()
        })
        .sum::<usize>();
    let indexes = details
        .indexes
        .iter()
        .map(|index| {
            std::mem::size_of::<IndexDetails>()
                + index.name.len()
                + index
                    .columns
                    .iter()
                    .map(|column| std::mem::size_of::<String>() + column.len())
                    .sum::<usize>()
                + index.definition.as_ref().map_or(0, String::len)
        })
        .sum::<usize>();
    let keys = details
        .foreign_keys
        .iter()
        .map(|key| {
            std::mem::size_of::<ForeignKeyDetails>()
                + key.name.len()
                + key.referenced_schema.len()
                + key.referenced_table.len()
                + key.on_update.len()
                + key.on_delete.len()
                + key
                    .columns
                    .iter()
                    .chain(&key.referenced_columns)
                    .map(|column| std::mem::size_of::<String>() + column.len())
                    .sum::<usize>()
        })
        .sum::<usize>();
    columns.saturating_add(indexes).saturating_add(keys)
}

fn action(value: &str) -> String {
    match value {
        "a" => "NO ACTION",
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        other => other,
    }
    .into()
}

// Literal escaping matches describe(); MySQL hex literals avoid SQL-mode-dependent escapes.
fn literal(engine: Engine, text: &str) -> String {
    match engine {
        Engine::MySql => format!(
            "CONVERT(X'{}' USING utf8mb4)",
            text.as_bytes()
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        ),
        Engine::Postgres => format!("E'{}'", text.replace('\\', "\\\\").replace('\'', "''")),
        _ => format!("'{}'", text.replace('\'', "''")),
    }
}
