use super::*;
use gpui_kit::base::Disableable;
use gpui_kit::component::{ActiveTheme, checkbox::Checkbox, input::Input, v_flex};
use gpui_kit::{Render, div, px};

struct CellForm {
    value: Entity<InputState>,
    null: bool,
}

struct InsertField {
    column: ColumnDetails,
    value: Entity<InputState>,
    include: bool,
    null: bool,
}

struct InsertForm {
    fields: Vec<InsertField>,
    error: Option<String>,
}

impl Render for InsertForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Unchecked columns are omitted to use database defaults or generated values. The database checks required values when you run the SQL. Values are literal text converted by the column type; use SQL for expressions."))
            .child(v_flex().id("insert-fields").max_h(px(360.)).overflow_y_scroll().gap_3().children(self.fields.iter().enumerate().map(|(index, field)| {
                v_flex().gap_2()
                    .child(div().text_sm().child(format!("{} · {}{}", field.column.name, field.column.data_type, if field.column.primary_key { " · Primary key" } else { "" })))
                    .child(h_flex().gap_3()
                        .child(Checkbox::new(("insert-include", index)).label("Include value").checked(field.include).on_click(cx.listener(move |this, checked, _, cx| {
                            this.fields[index].include = *checked;
                            cx.notify();
                        })))
                        .child(Checkbox::new(("insert-null", index)).label("SQL NULL").checked(field.null).disabled(!field.include || !field.column.nullable).on_click(cx.listener(move |this, checked, _, cx| {
                            this.fields[index].null = *checked;
                            cx.notify();
                        }))))
                    .child(Input::new(&field.value).disabled(!field.include || field.null).aria_label(format!("Value for {}", field.column.name)))
            })))
            .when_some(self.error.clone(), |view, error| view.child(div().text_sm().text_color(cx.theme().danger).child(error)))
    }
}

struct DeleteForm {
    confirmed: bool,
    error: bool,
}

impl Render for DeleteForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().gap_3()
            .child(div().text_sm().child("Prepare a DELETE for the selected record. The primary key and all original values protect against stale data. Running the SQL can invoke triggers and cascading foreign keys."))
            .child(Checkbox::new("delete-confirm").label("I understand that running this SQL deletes the record").checked(self.confirmed).on_click(cx.listener(|this, checked, _, cx| {
                this.confirmed = *checked;
                cx.notify();
            })))
            .when(self.error, |view| view.child(div().text_sm().text_color(cx.theme().danger).child("Confirm the checkbox before preparing the DELETE preview.")))
    }
}
impl Render for CellForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let null_checkbox = Checkbox::new("edit-null")
            .label("Set SQL NULL")
            .checked(self.null)
            .on_click(cx.listener(|this, value, _, cx| {
                this.null = *value;
                cx.notify();
            }));
        let explanation=div().text_xs().text_color(cx.theme().muted_foreground)
            .child("Prepare an UPDATE for review. Run the generated SQL to apply it. The WHERE clause checks the primary key and original values; zero affected rows means the record changed.");
        v_flex()
            .gap_3()
            .child(
                Input::new(&self.value)
                    .disabled(self.null)
                    .aria_label("New cell value"),
            )
            .child(null_checkbox)
            .child(explanation)
    }
}

impl Studio {
    pub(super) fn insert_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_row_operation(false, window, cx);
    }

    pub(super) fn delete_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_row_operation(true, window, cx);
    }

    fn prepare_row_operation(
        &mut self,
        deleting: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.any_running() || self.connecting {
            return;
        }
        if self.tabs.len() >= MAX_TABS {
            self.notice = "Close a query before preparing a record operation.".into();
            cx.notify();
            return;
        }
        let tab = &self.tabs[self.active];
        let Some(object) = tab
            .source_object
            .clone()
            .filter(|object| object.kind == crate::database::ObjectKind::Table)
        else {
            self.notice = "Open a base table's Data query to prepare a record operation.".into();
            cx.notify();
            return;
        };
        if tab.connection_id != self.active_connection
            || tab.executed_revision != Some(tab.revision)
        {
            self.notice =
                "Select the original connection and rerun the unchanged table query first.".into();
            cx.notify();
            return;
        }
        let selected = if deleting {
            let row = tab
                .results
                .read(cx)
                .selected_cell()
                .and_then(|(visible_row, _)| {
                    let result = tab.outputs.get(tab.result_index)?;
                    let displayed = tab.results.read(cx).delegate().rows.get(visible_row)?;
                    let row = result.rows.iter().position(|row| row == displayed)?;
                    // Keep only the selected original row in the modal snapshot.
                    Some(QueryResult {
                        columns: result.columns.clone(),
                        rows: vec![result.rows[row].clone()],
                        ..Default::default()
                    })
                });
            if row.is_none() {
                self.notice = "Select a result cell in the record to delete.".into();
                cx.notify();
                return;
            }
            row
        } else {
            None
        };
        let source_tab = tab.id;
        let source_revision = tab.revision;
        let Some(connection) = self.connection() else {
            return;
        };
        if connection.config.read_only {
            self.notice = "Record operations require a writable connection.".into();
            cx.notify();
            return;
        }
        let engine = connection.config.engine;
        let session = connection.session.clone();
        let connection_id = connection.config.id.clone();
        self.connecting = true;
        let work = cx
            .background_executor()
            .spawn(async move { session.describe(&object).map(|columns| (object, columns)) });
        self._catalog_task = Some(cx.spawn_in(window, async move |view, cx| {
            let described = work.await;
            let _ = cx.update_root(|_, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.connecting = false;
                    let (object, columns) = match described {
                        Ok(value) => value,
                        Err(error) => { this.notice = format!("Could not inspect the table: {error}"); cx.notify(); return; }
                    };
                    if !this.record_source_available(source_tab, source_revision, &connection_id, &object) {
                        this.notice = "The original table query or connection changed. Reopen the record operation.".into();
                        cx.notify();
                        return;
                    }
                    let delete_sql = match selected.as_ref().map(|result| crate::data_ops::delete_record_sql(engine, &object, &columns, result, 0)).transpose() {
                        Ok(sql) => sql,
                        Err(error) => { this.notice = error; cx.notify(); return; }
                    };
                    if columns.is_empty() || columns.len() > 128 {
                        this.notice = "Record forms support tables with 1–128 columns. Use explicit SQL for this table.".into();
                        cx.notify();
                        return;
                    }
                    let insert = cx.new(|cx| InsertForm {
                        fields: columns.iter().map(|column| InsertField { column: column.clone(), value: cx.new(|cx| InputState::new(window, cx)), include: false, null: false }).collect(),
                        error: None,
                    });
                    let delete = cx.new(|_| DeleteForm { confirmed: false, error: false });
                    let weak = cx.entity().downgrade();
                    window.open_dialog(cx, move |dialog, _, cx| {
                        let insert_for_save = insert.clone();
                        let delete_for_save = delete.clone();
                        let weak = weak.clone();
                        let object = object.clone();
                        let columns = columns.clone();
                        let connection_id = connection_id.clone();
                        let delete_sql = delete_sql.clone();
                        let cancel = Button::new("cancel-row-operation").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx));
                        let prepare = Button::new("prepare-row-operation")
                            .label(if deleting { "Prepare DELETE" } else { "Prepare INSERT" })
                            .on_click(move |_, window, cx| {
                                if deleting && !delete_for_save.read(cx).confirmed {
                                    delete_for_save.update(cx, |form, cx| { form.error = true; cx.notify(); });
                                    return;
                                }
                                let sql = if let Some(sql) = &delete_sql { Ok(sql.clone()) } else {
                                    let values = insert_for_save.read(cx).fields.iter().map(|field| {
                                        if !field.include { crate::data_ops::RecordValue::Default }
                                        else if field.null { crate::data_ops::RecordValue::Null }
                                        else { crate::data_ops::RecordValue::Text(field.value.read(cx).value().to_string()) }
                                    }).collect::<Vec<_>>();
                                    crate::data_ops::insert_record_sql(engine, &object, &columns, &values)
                                };
                                let sql = match sql {
                                    Ok(sql) => sql,
                                    Err(error) => { insert_for_save.update(cx, |form, cx| { form.error = Some(error); cx.notify(); }); return; }
                                };
                                if let Some(view) = weak.upgrade() { view.update(cx, |this, cx| {
                                    if this.tabs.len() >= MAX_TABS || !this.record_source_available(source_tab, source_revision, &connection_id, &object) {
                                        this.notice = "The original table query or connection changed, or the query limit was reached. Reopen the record operation.".into();
                                        cx.notify();
                                        return;
                                    }
                                    this.add_query(format!("{} {}", if deleting { "Delete" } else { "Insert" }, object.name), sql, window, cx);
                                    this.tabs[this.active].connection_id = Some(connection_id.clone());
                                    this.notice = if deleting { "DELETE prepared. Review triggers and cascades, then Run to apply. Expect one affected row; zero indicates stale data." } else { "INSERT prepared. Review values and defaults, then Run to apply." }.into();
                                    window.close_dialog(cx);
                                    cx.notify();
                                }); }
                            });
                        let prepare = if deleting { prepare.danger() } else { prepare.primary() };
                        dialogs::frame(dialog, "Table records", if deleting { "Review record deletion" } else { "Add a record" }, if deleting { "Prepare a guarded statement for one selected record." } else { "Set values or leave columns omitted for database defaults." }, 620., cx)
                            .child(if deleting { delete.clone().into_any_element() } else { insert.clone().into_any_element() })
                            .footer(h_flex().gap_2().justify_end().child(cancel).child(prepare))
                    });
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    fn record_source_available(
        &self,
        tab_id: usize,
        revision: usize,
        connection_id: &str,
        object: &DbObject,
    ) -> bool {
        !self.any_running()
            && !self.connecting
            && self.active_connection.as_deref() == Some(connection_id)
            && self
                .connection()
                .is_some_and(|connection| !connection.config.read_only)
            && self.tabs.iter().any(|tab| {
                tab.id == tab_id
                    && tab.revision == revision
                    && tab.executed_revision == Some(revision)
                    && tab.connection_id.as_deref() == Some(connection_id)
                    && tab.source_object.as_ref() == Some(object)
            })
    }

    pub(super) fn edit_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() >= MAX_TABS {
            self.notice = "Close a query before preparing a record edit.".into();
            cx.notify();
            return;
        }
        if self.any_running() || self.connecting {
            return;
        }
        let tab = &self.tabs[self.active];
        if tab.connection_id != self.active_connection
            || tab.executed_revision != Some(tab.revision)
        {
            self.notice="Select the original connection and rerun the unchanged table query before editing.".into();
            cx.notify();
            return;
        }
        let Some(object) = tab.source_object.clone() else {
            return;
        };
        let Some((visible_row, column)) = tab.results.read(cx).selected_cell() else {
            self.notice = "Select a result cell first.".into();
            cx.notify();
            return;
        };
        let Some(result) = tab.outputs.get(tab.result_index).cloned() else {
            return;
        };
        let Some(displayed) = tab.results.read(cx).delegate().rows.get(visible_row) else {
            return;
        };
        let Some(row) = result.rows.iter().position(|r| r == displayed) else {
            return;
        };
        let current = displayed.get(column).cloned().flatten();
        let Some(connection) = self.connection() else {
            return;
        };
        if connection.config.read_only {
            return;
        }
        let engine = connection.config.engine;
        let session = connection.session.clone();
        let connection_id = connection.config.id.clone();
        self.connecting = true;
        let work = cx
            .background_executor()
            .spawn(async move { session.describe(&object).map(|columns| (object, columns)) });
        self._catalog_task = Some(cx.spawn_in(window, async move |view, cx| {
            let described = work.await;
            let _ = cx.update_root(|_, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.connecting = false;
                    let (object, columns) = match described {
                        Ok(value) => value,
                        Err(error) => {
                            this.notice = format!("Could not inspect the record: {error}");
                            cx.notify();
                            return;
                        }
                    };
                    let form = cx.new(|cx| CellForm {
                        value: cx.new(|cx| {
                            InputState::new(window, cx)
                                .default_value(current.clone().unwrap_or_default())
                        }),
                        null: current.is_none(),
                    });
                    let weak = cx.entity().downgrade();
                    window.open_dialog(cx, move |dialog, _, cx| {
                        let form_for_save = form.clone();
                        let weak = weak.clone();
                        let object = object.clone();
                        let columns = columns.clone();
                        let result = result.clone();
                        let connection_id = connection_id.clone();
                        let cancel = Button::new("cancel-edit")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx));
                        let prepare = Button::new("prepare-update")
                            .primary()
                            .label("Prepare UPDATE")
                            .on_click(move |_, window, cx| {
                                let form = form_for_save.read(cx);
                                let value = form.value.read(cx).value().to_string();
                                let sql = crate::data_ops::edit_record_sql(
                                    engine,
                                    &object,
                                    &columns,
                                    &result,
                                    row,
                                    column,
                                    if form.null { None } else { Some(&value) },
                                );
                                if let Some(view) = weak.upgrade() {
                                    view.update(cx, |this, cx| match sql {
                                        Ok(sql) => {
                                            if this.tabs.len() >= MAX_TABS {
                                                this.notice = "Close a query before preparing a record edit.".into();
                                                window.close_dialog(cx);
                                                cx.notify();
                                                return;
                                            }
                                            this.add_query(
                                                format!("Update {}", object.name),
                                                sql,
                                                window,
                                                cx,
                                            );
                                            this.tabs[this.active].connection_id =
                                                Some(connection_id.clone());
                                            this.notice = "UPDATE prepared. Review it, then Run to apply. Affected rows must be 1; 0 indicates a stale record.".into();
                                            window.close_dialog(cx);
                                        }
                                        Err(error) => {
                                            this.notice = error;
                                            window.close_dialog(cx);
                                            cx.notify();
                                        }
                                    });
                                }
                            });
                        dialogs::frame(dialog, "Table records", "Edit a value", "Prepare an UPDATE statement to review before running.", 520., cx)
                            .child(form.clone())
                            .footer(h_flex().gap_2().justify_end().child(cancel).child(prepare))
                    });
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }
}
