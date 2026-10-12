use super::*;
use crate::database::{DbError, QueryOutput};
use crate::workspace::{HistoryRecord, MAX_HISTORY, new_id};
use std::time::{SystemTime, UNIX_EPOCH};

struct PendingQuery {
    tab_id: usize,
    revision: usize,
    connection_id: String,
    connection_name: String,
    source: String,
}

impl Studio {
    pub(super) fn format_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connection_id = self.tabs[self.active]
            .connection_id
            .as_ref()
            .or(self.active_connection.as_ref());
        let engine = connection_id
            .and_then(|id| {
                self.connections
                    .iter()
                    .find(|connection| &connection.config.id == id)
                    .map(|connection| connection.config.engine)
                    .or_else(|| {
                        self.workspace
                            .connections
                            .iter()
                            .find(|connection| &connection.id == id)
                            .map(|connection| connection.engine)
                    })
            })
            .unwrap_or(crate::database::Engine::SQLite);
        let source = self.tabs[self.active].editor.read(cx).value().to_string();
        match crate::sql_tools::format_sql(&source, engine) {
            Ok(formatted) => {
                if formatted != source {
                    let tab = &mut self.tabs[self.active];
                    tab.editor_source = formatted.clone();
                    tab.editor
                        .update(cx, |state, cx| state.set_value(formatted, window, cx));
                    tab.dirty = true;
                    tab.revision += 1;
                    tab.source_object = None;
                    tab.browse = None;
                }
            }
            Err(error) => self.notice = error,
        }
        cx.notify();
    }
    fn query_source(&self, cx: &App) -> String {
        let editor = self.tabs[self.active].editor.read(cx);
        let selected = editor.selected_text().to_string();
        if selected.trim().is_empty() {
            editor.value().to_string()
        } else {
            selected
        }
    }

    pub(super) fn run_query(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.execute_sql(self.query_source(cx), cx);
    }

    pub(super) fn run_statement(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(engine) = self.connection().map(|connection| connection.config.engine) else {
            self.notice = "Connect a database before running a statement.".into();
            cx.notify();
            return;
        };
        let editor = self.tabs[self.active].editor.read(cx);
        let source = editor.value().to_string();
        let offset = editor.cursor();
        match crate::sql_tools::statement_at_cursor(&source, offset, engine) {
            Ok(statement) => self.execute_sql(statement, cx),
            Err(error) => {
                self.notice = error;
                cx.notify();
            }
        }
    }

    pub(super) fn explain_query(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.connection().map(|connection| connection.config.engine) else {
            return;
        };
        match crate::database::explain_sql(&self.query_source(cx), engine) {
            Ok(sql) => self.execute_sql(sql, cx),
            Err(error) => {
                self.notice = error.to_string();
                cx.notify();
            }
        }
    }

    pub(super) fn execute_sql(&mut self, source: String, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting {
            return;
        }
        if self.tabs[self.active]
            .connection_id
            .as_ref()
            .is_some_and(|id| Some(id) != self.active_connection.as_ref())
        {
            self.notice = "This query belongs to a different connection. Select its connection, or explicitly Retarget this query before running.".into();
            cx.notify();
            return;
        }
        let Some(connection) = self.connection() else {
            self.notice = "Connect a database or create a scratch database first.".into();
            cx.notify();
            return;
        };
        if source.trim().is_empty() || source.len() > 512 * 1024 {
            self.notice = "Enter SQL before running. Queries are limited to 512 KiB.".into();
            cx.notify();
            return;
        }
        let session = connection.session.clone();
        let pending = PendingQuery {
            tab_id: self.tabs[self.active].id,
            revision: self.tabs[self.active].revision,
            connection_id: connection.config.id.clone(),
            connection_name: connection.config.name.clone(),
            source,
        };
        // Browse pages must load their full declared size so OFFSET does not skip rows.
        let row_limit = self.tabs[self.active]
            .browse
            .as_ref()
            .map_or(self.workspace.preferences.row_limit, |browse| {
                browse.request.limit
            });
        let tab = &mut self.tabs[self.active];
        // Selected statements and EXPLAIN results cannot retain table-row provenance.
        if pending.source.trim() != tab.editor.read(cx).value().trim() {
            tab.source_object = None;
            tab.browse = None;
        }
        let cancel = CancelToken::new();
        tab.cancel = Some(cancel.clone());
        tab.connection_id = Some(pending.connection_id.clone());
        tab.message = format!("Running on {}…", pending.connection_name);
        tab.is_error = false;
        self.notice.clear();
        self.show_results = true;
        let work = cx.background_executor().spawn(async move {
            let result = session.execute(&pending.source, row_limit, &cancel);
            (pending, result)
        });
        tab._task = Some(cx.spawn(async move |view, cx| {
            let (pending, result) = work.await;
            let _ = view.update(cx, |this, cx| this.finish_query(pending, result, cx));
        }));
        cx.notify();
    }

    fn finish_query(
        &mut self,
        pending: PendingQuery,
        result: Result<QueryOutput, DbError>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == pending.tab_id) else {
            return;
        };
        let tab = &mut self.tabs[index];
        tab.cancel = None;
        let (succeeded, elapsed_ms, row_count, message) = match result {
            Ok(output) => {
                tab.elapsed_ms = output.elapsed_ms;
                tab.executed_revision = Some(pending.revision);
                let row_count = output.results.iter().map(|result| result.rows.len()).sum();
                let affected = output
                    .results
                    .iter()
                    .map(|result| result.affected_rows)
                    .sum::<u64>();
                let truncated = output.results.iter().any(|result| result.truncated);
                let message = format!(
                    "{} · {} ms · {row_count} rows · {affected} affected{}{}",
                    pending.connection_name,
                    output.elapsed_ms,
                    if truncated {
                        " · Row limit reached; more rows exist."
                    } else {
                        ""
                    },
                    if tab.revision != pending.revision {
                        " · Editor changed during execution."
                    } else {
                        ""
                    }
                );
                tab.outputs = output.results;
                tab.result_index = 0;
                let mut delegate = tab
                    .outputs
                    .first()
                    .map(QueryResults::from_result)
                    .unwrap_or_else(QueryResults::empty);
                delegate.filter(self.result_filter.read(cx).value().as_str());
                tab.results.update(cx, |state, cx| {
                    *state.delegate_mut() = delegate;
                    state.refresh(cx);
                });
                tab.message = message.clone();
                tab.is_error = false;
                if self.active == index {
                    self.output_tab = OutputTab::Results;
                }
                (true, output.elapsed_ms, row_count, message)
            }
            Err(error) => {
                let message = error.to_string();
                tab.message = message.clone();
                tab.is_error = true;
                tab.elapsed_ms = 0;
                tab.outputs.clear();
                tab.executed_revision = None;
                tab.results.update(cx, |state, cx| {
                    *state.delegate_mut() = QueryResults::empty();
                    state.refresh(cx);
                });
                if self.active == index {
                    self.output_tab = OutputTab::Messages;
                }
                (false, 0, 0, message)
            }
        };
        self.workspace.history.push(HistoryRecord {
            id: new_id("history"),
            connection_id: pending.connection_id,
            connection_name: pending.connection_name,
            sql: pending.source,
            elapsed_ms,
            row_count,
            succeeded,
            message,
            at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default(),
        });
        if self.workspace.history.len() > MAX_HISTORY {
            self.workspace.history.remove(0);
        }
        self.persist();
        cx.notify();
    }

    pub(super) fn stop_query(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.tabs[self.active].cancel {
            cancel.cancel();
            self.tabs[self.active].message =
                "Cancellation requested; waiting for the database to stop.".into();
            cx.notify();
        }
    }

    pub(super) fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting {
            return;
        }
        let Some(connection) = self.connection() else {
            return;
        };
        let id = connection.config.id.clone();
        let session = connection.session.clone();
        self.connecting = true;
        let work = cx
            .background_executor()
            .spawn(async move { session.catalog() });
        self._catalog_task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |this, cx| {
                this.connecting = false;
                match result {
                    Ok(catalog) => {
                        if let Some(connection) = this
                            .connections
                            .iter_mut()
                            .find(|connection| connection.config.id == id)
                        {
                            connection.catalog = catalog;
                        }
                        this.rebuild_tree(cx);
                    }
                    Err(error) => this.notice = format!("Catalog refresh failed: {error}"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn inspect_object(&mut self, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting {
            return;
        }
        let (Some(object), Some(connection)) = (self.selected_object(cx), self.connection()) else {
            return;
        };
        let session = connection.session.clone();
        let id = connection.config.id.clone();
        let title = format!("{}.{}", object.schema, object.name);
        self.connecting = true;
        let work = cx
            .background_executor()
            .spawn(async move { session.table_details(&object) });
        self._catalog_task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |this, cx| {
                this.connecting = false;
                if this.active_connection.as_ref() != Some(&id) {
                    return;
                }
                match result {
                    Ok(details) => {
                        this.schema = details.columns;
                        this.schema_indexes = details.indexes;
                        this.schema_foreign_keys = details.foreign_keys;
                        this.schema_title = title;
                        this.show_results = true;
                        this.output_tab = OutputTab::Schema;
                        this.refresh_completions(cx);
                    }
                    Err(error) => this.notice = format!("Schema inspection failed: {error}"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
