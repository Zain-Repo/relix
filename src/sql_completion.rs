//! Local SQL suggestions use catalog metadata; no editor text leaves the app.

use std::collections::HashSet;

use gpui_kit::component::input::{CompletionProvider, Rope};
use gpui_kit::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, Position, Range,
    TextEdit,
};
use sqlparser::{
    dialect::{Dialect, DuckDbDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect},
    tokenizer::{Token, Tokenizer},
};

use crate::database::{ColumnDetails, DbObject, Engine};

struct Suggestion {
    label: String,
    insert: String,
    kind: CompletionItemKind,
    detail: String,
}

pub struct SqlCompletion {
    engine: Engine,
    suggestions: Vec<Suggestion>,
}

impl SqlCompletion {
    pub fn new(engine: Engine, objects: &[DbObject], columns: &[ColumnDetails]) -> Self {
        let keywords = [
            "SELECT",
            "FROM",
            "WHERE",
            "ORDER BY",
            "GROUP BY",
            "HAVING",
            "LIMIT",
            "OFFSET",
            "JOIN",
            "LEFT JOIN",
            "INNER JOIN",
            "ON",
            "AS",
            "AND",
            "OR",
            "NOT",
            "IS NULL",
            "DISTINCT",
            "COUNT",
            "SUM",
            "AVG",
            "MIN",
            "MAX",
            "INSERT INTO",
            "VALUES",
            "UPDATE",
            "SET",
            "DELETE FROM",
            "CREATE TABLE",
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "WITH",
            "UNION ALL",
        ];
        let mut suggestions = keywords
            .into_iter()
            .map(|keyword| Suggestion {
                label: keyword.into(),
                insert: keyword.into(),
                kind: CompletionItemKind::KEYWORD,
                detail: "SQL keyword".into(),
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        for object in objects.iter().take(2048) {
            let insert = if object.schema.is_empty() {
                engine.quote_identifier(&object.name)
            } else {
                format!(
                    "{}.{}",
                    engine.quote_identifier(&object.schema),
                    engine.quote_identifier(&object.name)
                )
            };
            if seen.insert(insert.clone()) {
                suggestions.push(Suggestion {
                    label: object.name.clone(),
                    insert,
                    kind: CompletionItemKind::CLASS,
                    detail: format!("{}.{}", object.schema, object.name),
                });
            }
        }
        for column in columns.iter().take(1024) {
            let insert = engine.quote_identifier(&column.name);
            if seen.insert(insert.clone()) {
                suggestions.push(Suggestion {
                    label: column.name.clone(),
                    insert,
                    kind: CompletionItemKind::FIELD,
                    detail: column.data_type.clone(),
                });
            }
        }
        Self {
            engine,
            suggestions,
        }
    }

    fn items(&self, source: &str, offset: usize) -> Vec<CompletionItem> {
        let Some((start, query)) = current_word(source, offset, self.engine) else {
            return Vec::new();
        };
        let range = Range::new(position(source, start), position(source, offset));
        self.suggestions
            .iter()
            .filter(|item| item.label.to_lowercase().starts_with(&query))
            .take(64)
            .map(|item| CompletionItem {
                label: item.label.clone(),
                kind: Some(item.kind),
                detail: Some(item.detail.clone()),
                filter_text: Some(item.label.clone()),
                text_edit: Some(TextEdit::new(range, item.insert.clone()).into()),
                ..Default::default()
            })
            .collect()
    }
}

impl CompletionProvider for SqlCompletion {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        Task::ready(Ok(CompletionResponse::Array(
            self.items(&text.to_string(), offset),
        )))
    }

    fn is_completion_trigger(&self, _: usize, inserted: &str, _: &mut App) -> bool {
        inserted
            .chars()
            .last()
            .is_some_and(|character| character.is_alphanumeric() || character == '_')
    }
}

fn current_word(source: &str, offset: usize, engine: Engine) -> Option<(usize, String)> {
    if source.len() > 512 * 1024 || !source.is_char_boundary(offset) {
        return None;
    }
    let prefix = source.get(..offset)?;
    // An incomplete quoted token is rejected by the same dialect lexer used
    // for scripts. Comments and completed quoted tokens are not word contexts.
    let dialect: Box<dyn Dialect> = match engine {
        Engine::SQLite => Box::new(SQLiteDialect {}),
        Engine::DuckDB => Box::new(DuckDbDialect {}),
        Engine::Postgres => Box::new(PostgreSqlDialect {}),
        Engine::MySql => Box::new(MySqlDialect {}),
    };
    let tokens = Tokenizer::new(dialect.as_ref(), prefix).tokenize().ok()?;
    if !matches!(tokens.last(), Some(Token::Word(word)) if word.quote_style.is_none()) {
        return None;
    }
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_alphanumeric() && *character != '_')
        .map_or(0, |(index, character)| index + character.len_utf8());
    if start == offset {
        return None;
    }
    Some((start, prefix[start..].to_lowercase()))
}

fn position(source: &str, offset: usize) -> Position {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let character = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .encode_utf16()
        .count() as u32;
    Position::new(line, character)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ObjectKind;

    #[test]
    fn suggestions_quote_metadata_and_replace_the_full_unicode_word() {
        let provider = SqlCompletion::new(
            Engine::SQLite,
            &[DbObject {
                schema: "main".into(),
                name: "café\"table".into(),
                kind: ObjectKind::Table,
            }],
            &[],
        );
        let sql = "SELECT * FROM café";
        let items = provider.items(sql, sql.len());
        assert_eq!(items.len(), 1);
        let Some(lsp_types::CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
            panic!("text edit required");
        };
        assert_eq!(edit.new_text, "\"main\".\"café\"\"table\"");
        assert_eq!(edit.range.start.character, 14);
        assert_eq!(edit.range.end.character, 18);
        let quoted = "SELECT 'unclosed";
        assert_eq!(current_word(quoted, quoted.len(), Engine::SQLite), None);
        assert_eq!(current_word("-- comment", 10, Engine::SQLite), None);
        assert_eq!(current_word("/* block", 8, Engine::SQLite), None);
        let dollar_quoted = "SELECT $$unclosed";
        assert_eq!(
            current_word(dollar_quoted, dollar_quoted.len(), Engine::Postgres),
            None
        );
    }
}
