//! Editor SQL tools preserve the source used for execution. A dialect-aware token comparison
//! rejects formatter output that changes literals, identifiers or comments.

use crate::database::Engine;
use sqlparser::{
    dialect::{Dialect, DuckDbDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect},
    tokenizer::{Location, Token, Tokenizer, Whitespace},
};

/// Returns the original SQL statement containing a UTF-8 byte cursor offset.
///
/// A cursor on a semicolon selects the preceding statement. Whitespace after a
/// separator selects the following statement; at EOF, trailing whitespace selects
/// the last statement. Empty segments and comments alone are not executable.
///
/// # Errors
/// Rejects invalid offsets, malformed tokens, and compound procedural bodies
/// whose internal semicolons cannot safely be treated as statement separators.
pub fn statement_at_cursor(
    source: &str,
    byte_offset: usize,
    engine: Engine,
) -> Result<String, String> {
    if !source.is_char_boundary(byte_offset) {
        return Err("The editor cursor is outside a valid SQL character boundary.".into());
    }
    let dialect = sql_dialect(engine);
    let tokens = Tokenizer::new(dialect.as_ref(), source)
        .tokenize_with_location()
        .map_err(|_| "The SQL contains an unterminated or invalid token.".to_string())?;

    // Server procedural languages have their own block grammars. Executing a
    // prefix of a trigger or routine would be surprising, so require selection
    // or full-script execution instead of guessing its internal boundaries.
    let mut first_word = true;
    let mut previous_begin = false;
    for token in &tokens {
        if let Token::Word(word) = &token.token {
            if word.quote_style.is_none()
                && ((word.value.eq_ignore_ascii_case("BEGIN") && !first_word)
                    || (word.value.eq_ignore_ascii_case("ATOMIC") && previous_begin))
            {
                return Err("Select the complete compound SQL body or run the full script.".into());
            }
            previous_begin = word.quote_style.is_none() && word.value.eq_ignore_ascii_case("BEGIN");
            first_word = false;
        } else if !matches!(token.token, Token::Whitespace(_)) {
            previous_begin = false;
        }
        if token.token == Token::SemiColon {
            first_word = true;
        }
    }

    let mut chars = source.char_indices().peekable();
    let mut location = Location::new(1, 1);
    let mut start = 0;
    let mut has_sql = false;
    let mut last_statement = None;
    for token in tokens {
        if token.token != Token::SemiColon {
            has_sql |= !matches!(token.token, Token::Whitespace(_) | Token::EOF);
            continue;
        }
        while location < token.span.start {
            let Some((_, character)) = chars.next() else {
                return Err("The SQL statement boundary could not be resolved safely.".into());
            };
            if character == '\n' {
                location.line += 1;
                location.column = 1;
            } else {
                location.column += 1;
            }
        }
        let Some(&(separator, ';')) = chars.peek() else {
            // MySQL executable comment hints are expanded into tokens sharing
            // the comment span. Their semicolons are not document separators.
            continue;
        };
        let end = separator + 1;
        if byte_offset <= separator {
            return executable_segment(source, start, end, has_sql);
        }
        if has_sql {
            last_statement = Some((start, end));
        }
        start = end;
        has_sql = false;
    }
    if has_sql {
        return executable_segment(source, start, source.len(), true);
    }
    if byte_offset == source.len()
        && source[start..].trim().is_empty()
        && let Some((start, end)) = last_statement
    {
        return executable_segment(source, start, end, true);
    }
    Err("There is no executable statement at the cursor.".into())
}

fn executable_segment(
    source: &str,
    start: usize,
    end: usize,
    has_sql: bool,
) -> Result<String, String> {
    if !has_sql {
        return Err("There is no executable statement at the cursor.".into());
    }
    Ok(source[start..end].trim().to_owned())
}

pub fn format_sql(source: &str, engine: Engine) -> Result<String, String> {
    if source.len() > 512 * 1024 {
        return Err("SQL formatting is limited to 512 KiB.".into());
    }
    let options = sqlformat::FormatOptions {
        dialect: if engine == Engine::Postgres {
            sqlformat::Dialect::PostgreSql
        } else {
            sqlformat::Dialect::Generic
        },
        ..Default::default()
    };
    let formatted = sqlformat::format(source, &sqlformat::QueryParams::None, &options);
    if significant_tokens(source, engine)? != significant_tokens(&formatted, engine)? {
        return Err("This SQL construct could not be formatted without changing its tokens. The original query is preserved.".into());
    }
    Ok(formatted)
}

fn significant_tokens(source: &str, engine: Engine) -> Result<Vec<Token>, String> {
    let dialect = sql_dialect(engine);
    Tokenizer::new(dialect.as_ref(), source)
        .tokenize()
        .map(|tokens| {
            tokens
                .into_iter()
                .filter(|token| {
                    !matches!(
                        token,
                        Token::Whitespace(
                            Whitespace::Space | Whitespace::Newline | Whitespace::Tab
                        ) | Token::EOF
                    )
                })
                .collect()
        })
        .map_err(|_| {
            "The SQL contains an unterminated or invalid token. The original query is preserved."
                .into()
        })
}

fn sql_dialect(engine: Engine) -> Box<dyn Dialect> {
    match engine {
        Engine::SQLite => Box::new(SQLiteDialect {}),
        Engine::DuckDB => Box::new(DuckDbDialect {}),
        Engine::Postgres => Box::new(PostgreSqlDialect {}),
        Engine::MySql => Box::new(MySqlDialect {}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_execution_preserves_quoted_semicolons_and_comments() {
        for engine in Engine::ALL {
            let sql = "SELECT 'one;two', \"odd;name\";\n-- comment; retained\nSELECT 2 /* ; */;";
            let cursor = sql.find("SELECT 2").unwrap();
            assert_eq!(
                statement_at_cursor(sql, cursor, engine).unwrap(),
                "-- comment; retained\nSELECT 2 /* ; */;"
            );
        }
    }

    #[test]
    fn cursor_execution_obeys_separator_and_end_of_document_boundaries() {
        let sql = "SELECT 1; SELECT 2;  ";
        assert_eq!(
            statement_at_cursor(sql, 8, Engine::SQLite).unwrap(),
            "SELECT 1;"
        );
        assert_eq!(
            statement_at_cursor(sql, 9, Engine::SQLite).unwrap(),
            "SELECT 2;"
        );
        assert_eq!(
            statement_at_cursor(sql, sql.len(), Engine::SQLite).unwrap(),
            "SELECT 2;"
        );
        assert_eq!(
            statement_at_cursor("SELECT 3", 8, Engine::SQLite).unwrap(),
            "SELECT 3"
        );
        assert!(statement_at_cursor(sql, 19, Engine::SQLite).is_err());
        assert!(statement_at_cursor("; SELECT 1", 0, Engine::SQLite).is_err());
    }

    #[test]
    fn cursor_execution_handles_utf8_and_crlf_without_changing_source() {
        let sql = "SELECT 'café;東京';\r\nSELECT '👋';";
        let cursor = sql.find("SELECT '👋'").unwrap();
        assert_eq!(
            statement_at_cursor(sql, cursor, Engine::SQLite).unwrap(),
            "SELECT '👋';"
        );
        let inside_character = sql.find('é').unwrap() + 1;
        assert!(statement_at_cursor(sql, inside_character, Engine::SQLite).is_err());
        assert!(statement_at_cursor(sql, sql.len() + 1, Engine::SQLite).is_err());
    }

    #[test]
    fn cursor_execution_handles_postgres_dollar_quoted_bodies() {
        let sql = "SELECT $body$BEGIN; 'text'; END$body$, $$more;sql$$; SELECT $1;";
        assert_eq!(
            statement_at_cursor(sql, 20, Engine::Postgres).unwrap(),
            "SELECT $body$BEGIN; 'text'; END$body$, $$more;sql$$;"
        );
        assert_eq!(
            statement_at_cursor(sql, sql.len(), Engine::Postgres).unwrap(),
            "SELECT $1;"
        );
    }

    #[test]
    fn cursor_execution_handles_mysql_backticks_escapes_and_comment_hints() {
        let sql = "SELECT `semi;colon`, 'it\\'s;quoted' /*! ; */; SELECT 2;";
        assert_eq!(
            statement_at_cursor(sql, 10, Engine::MySql).unwrap(),
            "SELECT `semi;colon`, 'it\\'s;quoted' /*! ; */;"
        );
        assert_eq!(
            statement_at_cursor(sql, sql.len(), Engine::MySql).unwrap(),
            "SELECT 2;"
        );
    }

    #[test]
    fn cursor_execution_rejects_invalid_empty_and_compound_sql() {
        for sql in [
            "",
            "  ",
            "-- comment;",
            "/* comment */",
            "SELECT 'unterminated",
        ] {
            assert!(statement_at_cursor(sql, sql.len(), Engine::SQLite).is_err());
        }
        let trigger =
            "CREATE TRIGGER t AFTER INSERT ON items BEGIN INSERT INTO log VALUES (1); END;";
        assert!(statement_at_cursor(trigger, 0, Engine::SQLite).is_err());
        assert!(statement_at_cursor("BEGIN ATOMIC SELECT 1; END;", 0, Engine::Postgres).is_err());
        assert_eq!(
            statement_at_cursor("BEGIN; SELECT 1; COMMIT;", 0, Engine::Postgres).unwrap(),
            "BEGIN;"
        );
    }

    #[test]
    fn formatting_preserves_literals_parameters_identifiers_and_comments() {
        for engine in Engine::ALL {
            let sql = "SELECT 'semi;colon' AS value, \"odd name\" FROM items WHERE id = 42; /* Keep this note */";
            let formatted = format_sql(sql, engine).expect("format common SQL");
            assert!(formatted.contains('\n'));
            assert_eq!(
                significant_tokens(sql, engine).unwrap(),
                significant_tokens(&formatted, engine).unwrap()
            );
        }
        let sql = "SELECT $1, $$body;value$$, ARRAY[1,2];";
        if let Ok(formatted) = format_sql(sql, Engine::Postgres) {
            assert_eq!(
                significant_tokens(sql, Engine::Postgres).unwrap(),
                significant_tokens(&formatted, Engine::Postgres).unwrap()
            );
        }
    }

    #[test]
    fn invalid_or_oversized_sql_is_not_rewritten() {
        assert!(format_sql("SELECT 'unterminated", Engine::SQLite).is_err());
        assert!(format_sql(&" ".repeat(512 * 1024 + 1), Engine::SQLite).is_err());
    }
}
