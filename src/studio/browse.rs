//! Table browsing prepares bounded, reviewable SQL without executing it.

use super::*;
use crate::database::Engine;
use crate::table_browse::{BrowseFilter, BrowseRequest, BrowseSort, FilterOperator, browse_sql};
use gpui_kit::component::{
    ActiveTheme, Selectable, Sizable, checkbox::Checkbox, input::Input, v_flex,
};
use gpui_kit::{Render, div};

struct BrowseForm {
    columns: Vec<ColumnDetails>,
    filter_enabled: bool,
    filter_column: Entity<InputState>,
    filter_value: Entity<InputState>,
    operator: FilterOperator,
    sort_column: Entity<InputState>,
    descending: bool,
    limit: Entity<InputState>,
    page: Entity<InputState>,
    error: String,
}

impl BrowseForm {
    fn request(&self, object: DbObject, cx: &App) -> Result<BrowseRequest, String> {
        let limit = self
            .limit
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "Rows per page must be a whole number.".to_string())?;
        let page = self
            .page
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "Page must be a positive whole number.".to_string())?
            .checked_sub(1)
            .ok_or("Page starts at 1.")?;
        let sort_column = self.sort_column.read(cx).value().to_string();
        Ok(BrowseRequest {
            object,
            filter: self.filter_enabled.then(|| BrowseFilter {
                column: self.filter_column.read(cx).value().to_string(),
                operator: self.operator,
                value: self.filter_value.read(cx).value().to_string(),
            }),
            sort: (!sort_column.is_empty()).then_some(BrowseSort {
                column: sort_column,
                descending: self.descending,
            }),
            limit,
            page,
        })
    }
}

impl Render for BrowseForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let field = |label: &'static str, state: &Entity<InputState>| {
            v_flex()
                .gap_1()
                .child(div().text_xs().child(label))
                .child(Input::new(state).aria_label(label))
        };
        v_flex().gap_3()
            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                .child(format!("Columns: {}", self.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>().join(", "))))
            .child(Checkbox::new("browse-enable-filter").label("Filter database rows")
                .checked(self.filter_enabled).on_click(cx.listener(|this, value, _, cx| {
                    this.filter_enabled = *value; cx.notify();
                })))
            .when(self.filter_enabled, |element| element
                .child(field("Filter column (exact name)", &self.filter_column))
                .child(h_flex().flex_wrap().gap_1().children([
                    (FilterOperator::Equals, "Equals"),
                    (FilterOperator::NotEquals, "Not equal"),
                    (FilterOperator::Contains, "Contains"),
                    (FilterOperator::IsNull, "Is NULL"),
                    (FilterOperator::IsNotNull, "Not NULL"),
                ].into_iter().enumerate().map(|(index, (operator, label))| {
                    Button::new(("browse-operator", index)).small().label(label)
                        .selected(self.operator == operator)
                        .on_click(cx.listener(move |this, _, _, cx| { this.operator = operator; cx.notify(); }))
                })))
                .when(!matches!(self.operator, FilterOperator::IsNull | FilterOperator::IsNotNull), |element| {
                    element.child(field("Filter value", &self.filter_value))
                }))
            .child(field("Order by column (blank for no ordering)", &self.sort_column))
            .child(Checkbox::new("browse-descending").label("Descending order")
                .checked(self.descending).on_click(cx.listener(|this, value, _, cx| {
                    this.descending = *value; cx.notify();
                })))
            .child(h_flex().gap_3()
                .child(div().flex_1().child(field("Rows per page (1–10,000)", &self.limit)))
                .child(div().flex_1().child(field("Page (starts at 1)", &self.page))))
            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                .child("Prepare a query, review its WHERE / ORDER BY / LIMIT, then Run. Use a unique ordering for predictable pages; rows can change between runs."))
            .when(!self.error.is_empty(), |element| element.child(div()
                .id("browse-error").role(gpui_kit::Role::Alert).child(self.error.clone())))
    }
}

impl Studio {
    pub(super) fn browse_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting || self.tabs.len() >= MAX_TABS {
            return;
        }
        let (Some(object), Some(connection)) = (self.selected_object(cx), self.connection()) else {
            return;
        };
        let session = connection.session.clone();
        let id = connection.config.id.clone();
        let engine = connection.config.engine;
        let inspected = object.clone();
        self.connecting = true;
        let work = cx
            .background_executor()
            .spawn(async move { session.describe(&inspected) });
        self._catalog_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = cx.update_root(|_, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.connecting = false;
                    if this.active_connection.as_ref() != Some(&id) {
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(columns) => {
                            this.open_browse_form(object, id, engine, columns, window, cx)
                        }
                        Err(error) => {
                            this.notice = format!("Could not inspect table columns: {error}")
                        }
                    }
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    fn open_browse_form(
        &mut self,
        object: DbObject,
        connection_id: String,
        engine: Engine,
        columns: Vec<ColumnDetails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_limit = self.workspace.preferences.row_limit;
        let default_column = columns
            .first()
            .map(|column| column.name.clone())
            .unwrap_or_default();
        let default_sort = columns
            .iter()
            .find(|column| column.primary_key)
            .map(|column| column.name.clone())
            .unwrap_or_default();
        let form = cx.new(|cx| BrowseForm {
            columns,
            filter_enabled: false,
            filter_column: cx.new(|cx| InputState::new(window, cx).default_value(default_column)),
            filter_value: cx.new(|cx| InputState::new(window, cx)),
            operator: FilterOperator::Equals,
            sort_column: cx.new(|cx| InputState::new(window, cx).default_value(default_sort)),
            descending: false,
            limit: cx.new(|cx| InputState::new(window, cx).default_value(row_limit.to_string())),
            page: cx.new(|cx| InputState::new(window, cx).default_value("1")),
            error: String::new(),
        });
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let form = form.clone(); let submit = form.clone();
            let view = view.clone(); let object = object.clone(); let connection_id = connection_id.clone();
            dialogs::frame(dialog, "Table browser", "Shape your result", "Filter, order and page the table with reviewable SQL.", 620., cx).child(form)
                .footer(h_flex().gap_2().justify_end()
                    .child(Button::new("browse-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("browse-prepare").primary().label("Prepare query").on_click(move |_, window, cx| {
                        let result = submit.read(cx).request(object.clone(), cx)
                            .and_then(|request| browse_sql(engine, &request, &submit.read(cx).columns).map(|sql| (request, sql)));
                        let Some(view) = view.upgrade() else { return; };
                        match result {
                            Ok((request, sql)) => {
                                let prepared = view.update(cx, |this, cx| {
                                    if this.any_running() || this.connecting || this.tabs.len() >= MAX_TABS
                                        || this.active_connection.as_ref() != Some(&connection_id) {
                                        this.notice = "Select the original connection and close a query if needed, then prepare again.".into();
                                        cx.notify(); return false;
                                    }
                                    this.add_query(format!("Browse {}", object.name), sql, window, cx);
                                    this.tabs[this.active].source_object = Some(object.clone());
                                    this.tabs[this.active].browse = Some(TableBrowseState {
                                        engine, request: request.clone(), columns: submit.read(cx).columns.clone(),
                                    });
                                    this.notice = "Table query prepared. Review and Run to load this page.".into();
                                    cx.notify(); true
                                });
                                if prepared { window.close_dialog(cx); }
                            }
                            Err(error) => submit.update(cx, |this, cx| { this.error = error; cx.notify(); }),
                        }
                    })))
        });
    }

    pub(super) fn change_browse_page(
        &mut self,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.tabs[self.active];
        if self.any_running()
            || self.connecting
            || tab.connection_id != self.active_connection
            || tab.executed_revision != Some(tab.revision)
        {
            return;
        }
        let Some(mut browse) = tab.browse.clone() else {
            return;
        };
        if next
            && tab
                .outputs
                .get(tab.result_index)
                .is_none_or(|result| result.rows.len() < browse.request.limit)
        {
            self.notice = "This page is incomplete or has reached the end. If its memory budget was exceeded, prepare a smaller page before continuing.".into();
            cx.notify();
            return;
        }
        let page = if next {
            browse.request.page.checked_add(1)
        } else {
            browse.request.page.checked_sub(1)
        };
        let Some(page) = page else {
            return;
        };
        browse.request.page = page;
        let sql = match browse_sql(browse.engine, &browse.request, &browse.columns) {
            Ok(sql) => sql,
            Err(error) => {
                self.notice = error;
                cx.notify();
                return;
            }
        };
        let tab = &mut self.tabs[self.active];
        tab.editor
            .update(cx, |state, cx| state.set_value(sql.clone(), window, cx));
        // Input change events may arrive after this callback. The SQL identity is
        // retained below and checked by the editor subscription before invalidation.
        tab.source_object = Some(browse.request.object.clone());
        tab.browse = Some(browse);
        tab.editor_source = sql.clone();
        tab.dirty = true;
        tab.revision += 1;
        self.execute_sql(sql, cx);
    }
}
