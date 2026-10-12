//! Preserve the user's exact SQL while finding statement boundaries with a
//! dialect-aware lexer. Semicolons inside strings, comments and dollar quotes
//! are never treated as boundaries.

use super::{DbError, Engine};
use sqlparser::{
    dialect::{Dialect, DuckDbDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect},
    tokenizer::{Location, Token, Tokenizer},
};

fn dialect(engine: Engine) -> Box<dyn Dialect> {
    match engine {
        Engine::SQLite => Box::new(SQLiteDialect {}),
        Engine::DuckDB => Box::new(DuckDbDialect {}),
        Engine::Postgres => Box::new(PostgreSqlDialect {}),
        Engine::MySql => Box::new(MySqlDialect {}),
    }
}

pub(super) fn split(sql: &str, engine: Engine) -> Result<Vec<String>, DbError> {
    if sql.len() > 2 * 1024 * 1024 {
        return Err(DbError("SQL scripts are limited to 2 MiB.".into()));
    }
    let dialect = dialect(engine);
    let tokens = Tokenizer::new(dialect.as_ref(), sql)
        .tokenize_with_location()
        .map_err(|error| DbError(error.to_string()))?;
    let mut statements = Vec::new();
    let mut start = 0;
    let mut has_content = false;
    let mut first_words = Vec::new();
    let mut body_stack: Vec<String> = Vec::new();
    for item in tokens {
        match item.token {
            Token::Whitespace(_) | Token::EOF => {}
            Token::SemiColon if body_stack.is_empty() => {
                let end = offset(sql, item.span.start);
                if has_content {
                    statements.push(sql[start..end].trim().to_owned());
                }
                start = end + 1;
                has_content = false;
                first_words.clear();
            }
            Token::Word(word) if word.quote_style.is_none() => {
                let value = word.value.to_ascii_uppercase();
                if first_words.len() < 4 {
                    first_words.push(value.clone());
                }
                // SQLite trigger bodies contain SQL statements; their END and
                // CASE ... END constructs must remain in one prepared statement.
                let compound = first_words.first().is_some_and(|word| word == "CREATE")
                    && first_words
                        .iter()
                        .any(|word| matches!(word.as_str(), "TRIGGER" | "PROCEDURE" | "FUNCTION"));
                if compound {
                    if value == "BEGIN" || (value == "CASE" && !body_stack.is_empty()) {
                        body_stack.push(value.clone());
                    } else if value == "END" {
                        body_stack.pop();
                    }
                }
                has_content = true;
            }
            _ => has_content = true,
        }
        if statements.len() > 1000 {
            return Err(DbError(
                "A script can contain at most 1,000 statements.".into(),
            ));
        }
    }
    if has_content {
        statements.push(sql[start..].trim().to_owned());
    }
    if statements.len() > 1000 {
        return Err(DbError(
            "A script can contain at most 1,000 statements.".into(),
        ));
    }
    Ok(statements)
}

fn offset(sql: &str, location: Location) -> usize {
    let mut line = 1;
    let mut column = 1;
    for (offset, character) in sql.char_indices() {
        if line == location.line && column == location.column {
            return offset;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    sql.len()
}

pub(super) fn first_word(sql: &str, engine: Engine) -> Result<String, DbError> {
    let dialect = dialect(engine);
    let tokens = Tokenizer::new(dialect.as_ref(), sql)
        .tokenize()
        .map_err(|error| DbError(error.to_string()))?;
    Ok(tokens
        .into_iter()
        .find_map(|token| match token {
            Token::Word(word) if word.quote_style.is_none() => {
                Some(word.value.to_ascii_uppercase())
            }
            Token::Whitespace(_) => None,
            _ => Some(String::new()),
        })
        .unwrap_or_default())
}

pub(super) fn command_and_returning(sql: &str, engine: Engine) -> Result<(String, bool), DbError> {
    let dialect = dialect(engine);
    let tokens = Tokenizer::new(dialect.as_ref(), sql)
        .tokenize()
        .map_err(|error| DbError(error.to_string()))?;
    let mut depth = 0_usize;
    let mut words = Vec::new();
    for token in tokens {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Word(word) if depth == 0 && word.quote_style.is_none() => {
                words.push(word.value.to_ascii_uppercase())
            }
            _ => {}
        }
    }
    let first = words.first().cloned().unwrap_or_default();
    let command = if first == "WITH" {
        words
            .iter()
            .skip(1)
            .find(|word| {
                matches!(
                    word.as_str(),
                    "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "VALUES"
                )
            })
            .cloned()
            .unwrap_or(first)
    } else {
        first
    };
    Ok((command, words.iter().any(|word| word == "RETURNING")))
}

pub(super) fn check_read_only_session(sql: &str, engine: Engine) -> Result<(), DbError> {
    // Writes are rejected by the database transaction itself. This guard only
    // prevents SQL from ending that transaction or changing its access mode.
    let keyword = first_word(sql, engine)?;
    if engine == Engine::MySql
        && !matches!(
            keyword.as_str(),
            "SELECT"
                | "WITH"
                | "EXPLAIN"
                | "SHOW"
                | "DESCRIBE"
                | "DESC"
                | "VALUES"
                | "TABLE"
                | "INSERT"
                | "UPDATE"
                | "DELETE"
                | "REPLACE"
        )
    {
        // MySQL procedures and administrative statements can implicitly end a
        // transaction. Keep those outside read-only sessions; the server's
        // READ ONLY transaction remains responsible for rejecting actual DML.
        return Err(DbError(
            "This command is unavailable on a read-only MySQL connection.".into(),
        ));
    }
    if matches!(
        keyword.as_str(),
        "BEGIN"
            | "START"
            | "COMMIT"
            | "END"
            | "ROLLBACK"
            | "ABORT"
            | "SAVEPOINT"
            | "RELEASE"
            | "SET"
            | "RESET"
            | "DISCARD"
            | "XA"
    ) {
        Err(DbError(
            "Session and transaction control is unavailable on a read-only connection.".into(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitting_preserves_quoted_semicolons_unicode_and_procedural_bodies() {
        let statements = split("SELECT 'é;'; -- ;\n SELECT $$a;b$$;", Engine::Postgres).unwrap();
        assert_eq!(statements, ["SELECT 'é;'", "-- ;\n SELECT $$a;b$$"]);
        let sql = "CREATE TRIGGER sample AFTER INSERT ON items BEGIN UPDATE items SET value = CASE WHEN 1 THEN 2 END; UPDATE items SET value=3; END; SELECT 1;";
        assert_eq!(split(sql, Engine::SQLite).unwrap().len(), 2);
        assert!(split("-- comment only", Engine::SQLite).unwrap().is_empty());
    }
    #[test]
    fn comments_cannot_hide_read_only_transaction_escape() {
        assert!(check_read_only_session("/* comment */ COMMIT", Engine::Postgres).is_err());
        assert!(check_read_only_session("SELECT 'COMMIT'", Engine::Postgres).is_ok());
    }
}
