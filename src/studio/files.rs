use super::*;
use crate::workspace::{MAX_SAVED_QUERIES, SavedQuery, new_id};
use gpui_kit::PathPromptOptions;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

impl Studio {
    pub(super) fn save_query(&mut self, cx: &mut Context<Self>) {
        if self.tabs[self.active].saved_id.is_none()
            && self.workspace.queries.len() >= MAX_SAVED_QUERIES
        {
            self.notice =
                "Saved query limit reached. Remove a saved query before adding another.".into();
            cx.notify();
            return;
        }
        let tab = &mut self.tabs[self.active];
        let source = tab.editor.read(cx).value().to_string();
        if source.len() > 512 * 1024 {
            self.notice = "Saved queries are limited to 512 KiB.".into();
            cx.notify();
            return;
        }
        let id = tab.saved_id.clone().unwrap_or_else(|| new_id("query"));
        self.workspace.queries.retain(|query| query.id != id);
        self.workspace.queries.push(SavedQuery {
            id: id.clone(),
            title: tab.title.clone(),
            sql: source,
            connection_id: tab.connection_id.clone(),
        });
        tab.saved_id = Some(id);
        self.notice = "Query saved to this workspace.".into();
        if self.persist() {
            self.tabs[self.active].dirty = false;
        }
        cx.notify();
    }

    pub(super) fn open_sql_file(
        &mut self,
        import_csv: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let engine = self.connection().map(|c| c.config.engine);
        let connection_id = self.active_connection.clone();
        if import_csv && (engine.is_none() || self.connection().is_some_and(|c| c.config.read_only))
        {
            self.notice = "Choose a writable connection to import CSV.".into();
            cx.notify();
            return;
        }
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                if import_csv {
                    "Choose CSV to prepare an import"
                } else {
                    "Open SQL file"
                }
                .into(),
            ),
        });
        let executor = cx.background_executor().clone();
        self._file_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = match prompt.await {
                Ok(Ok(Some(paths))) => {
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    let title = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or("Query.sql".into());
                    let table = path
                        .file_stem()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or("imported_data".into());
                    let result = executor
                        .spawn(async move {
                            read_text_bounded(
                                &path,
                                if import_csv { 1024 * 1024 } else { 512 * 1024 },
                            )
                        })
                        .await;
                    result.and_then(|text| {
                        if import_csv {
                            crate::data_ops::import_csv_sql(
                                engine.ok_or("No database selected")?,
                                &table,
                                &text,
                            )
                            .map(|sql| (format!("Import {table}"), sql))
                        } else {
                            Ok((title, text))
                        }
                    })
                }
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = cx.update_root(|_, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    match result {
                        Ok((title, sql)) => {
                            if this.tabs.len() >= MAX_TABS {
                                this.notice = "Close a query before opening another file.".into();
                            } else {
                                this.add_query(title, sql, window, cx);
                                if import_csv {
                                    this.tabs[this.active].connection_id = connection_id;
                                    this.refresh_completions(cx);
                                    this.notice = "Import SQL prepared. Review the table name and statements, then Run to import. No data has been changed yet.".into();
                                }
                            }
                        }
                        Err(error) => this.notice = format!("Could not open file: {error}"),
                    }
                    cx.notify();
                });
            });
        }));
    }

    pub(super) fn write_file(
        &mut self,
        format: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.tabs[self.active];
        let id = tab.id;
        let revision = tab.revision;
        let data = match format {
            "sql" => Ok(tab.editor.read(cx).value().to_string()),
            "csv" => tab
                .outputs
                .get(tab.result_index)
                .ok_or("No result to export".into())
                .and_then(crate::data_ops::result_to_csv),
            "json" => tab
                .outputs
                .get(tab.result_index)
                .ok_or("No result to export".into())
                .and_then(crate::data_ops::result_to_json),
            "md" => tab
                .outputs
                .get(tab.result_index)
                .ok_or("No result to export".into())
                .and_then(crate::data_ops::result_to_markdown),
            "insert" => (|| {
                if tab.executed_revision != Some(tab.revision) {
                    return Err(
                        "Rerun the unchanged table query before exporting INSERT SQL.".into(),
                    );
                }
                let object = tab
                    .source_object
                    .as_ref()
                    .ok_or("INSERT export requires a table browsing query.")?;
                let connection = tab
                    .connection_id
                    .as_ref()
                    .and_then(|id| {
                        self.connections
                            .iter()
                            .find(|connection| &connection.config.id == id)
                    })
                    .ok_or("Reconnect the original connection before exporting INSERT SQL.")?;
                let result = tab
                    .outputs
                    .get(tab.result_index)
                    .ok_or("No result to export")?;
                crate::data_ops::result_to_insert_sql(connection.config.engine, object, result)
            })(),
            _ => Err("Unsupported file format".into()),
        };
        let data = match data {
            Ok(data) => data,
            Err(error) => {
                self.notice = error;
                cx.notify();
                return;
            }
        };
        let directory = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let filename = format!(
            "{}.{}",
            if format == "sql" { "query" } else { "results" },
            if format == "insert" { "sql" } else { format }
        );
        let prompt = cx.prompt_for_new_path(&directory, Some(&filename));
        let executor = cx.background_executor().clone();
        self._file_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = match prompt.await {
                Ok(Ok(Some(path))) => {
                    executor
                        .spawn(async move { write_text_atomically(&path, &data) })
                        .await
                }
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = view.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.notice = format!("{format} file saved.");
                        if format == "sql"
                            && let Some(tab) = this
                                .tabs
                                .iter_mut()
                                .find(|t| t.id == id && t.revision == revision)
                        {
                            tab.dirty = false;
                        }
                    }
                    Err(error) => this.notice = format!("File save failed: {error}"),
                }
                cx.notify();
            });
        }));
    }
}

fn read_text_bounded(path: &Path, limit: u64) -> Result<String, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("File exceeds the {} KiB limit", limit / 1024));
    }
    String::from_utf8(bytes).map_err(|_| "File must contain UTF-8 text".into())
}

fn write_text_atomically(path: &Path, text: &str) -> Result<(), String> {
    let name = path.file_name().ok_or("Choose a file name")?;
    let mut temporary_name = name.to_os_string();
    temporary_name.push(format!(".{}.tmp", new_id("relix")));
    let temporary = path.with_file_name(temporary_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        // Cleanup is best-effort after the write failure has been reported;
        // the original destination remains intact until the final rename.
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_save_replaces_complete_content_and_preserves_destination_on_failure() {
        let directory = std::env::temp_dir().join(new_id("relix-file-test"));
        std::fs::create_dir(&directory).expect("Test directory must be created");
        let path = directory.join("query.sql");
        write_text_atomically(&path, "SELECT 1;").expect("Initial file must save");
        write_text_atomically(&path, "SELECT 2;").expect("Existing file must replace");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "SELECT 2;");
        let folder = directory.join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_text_atomically(&folder, "content").is_err());
        assert!(folder.is_dir());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
