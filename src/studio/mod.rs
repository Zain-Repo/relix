mod browse;
mod commands;
mod connection;
mod dialogs;
mod execution;
mod files;
mod navigator;
mod queries;
mod record;
mod results;
#[cfg(test)]
mod tests;
mod updates;
mod view;

use crate::database::{
    CancelToken, ColumnDetails, ConnectionConfig, DatabaseSession, DbObject, ForeignKeyDetails,
    IndexDetails, QueryResult,
};
use crate::workspace::{WorkspaceData, WorkspaceStore};
use gpui_kit::component::{
    WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{EditorState, InputEvent, InputState},
    table::TableState,
    tree::{TreeEvent, TreeState},
};
use gpui_kit::{
    App, Context, Entity, FocusHandle, KeyBinding, Subscription, Task, Window, prelude::*,
};
use results::QueryResults;

gpui_kit::actions!(
    studio,
    [
        NewQuery,
        RunQuery,
        RunStatement,
        ToggleSidebar,
        ToggleResults,
        SaveQuery,
        OpenQuery,
        SaveSqlFile,
        ExportCsv,
        ExportJson,
        ExportMarkdown,
        ExportInsert,
        FormatQuery,
        OpenCommands
    ]
);
const MAX_TABS: usize = 32;

pub fn bind_keys(cx: &mut App) {
    for modifier in ["ctrl", "cmd"] {
        cx.bind_keys([
            KeyBinding::new(&format!("{modifier}-k"), OpenCommands, Some("Studio")),
            KeyBinding::new(
                &format!("{modifier}-k"),
                OpenCommands,
                Some("Studio > Input"),
            ),
            KeyBinding::new(&format!("{modifier}-n"), NewQuery, Some("Studio")),
            KeyBinding::new(&format!("{modifier}-enter"), RunQuery, Some("Studio")),
            KeyBinding::new(
                &format!("{modifier}-shift-enter"),
                RunStatement,
                Some("Studio"),
            ),
            KeyBinding::new(
                &format!("{modifier}-shift-enter"),
                RunStatement,
                Some("Studio > Input"),
            ),
            KeyBinding::new(
                &format!("{modifier}-enter"),
                RunQuery,
                Some("Studio > Input"),
            ),
            KeyBinding::new(&format!("{modifier}-b"), ToggleSidebar, Some("Studio")),
            KeyBinding::new(&format!("{modifier}-j"), ToggleResults, Some("Studio")),
            KeyBinding::new(&format!("{modifier}-s"), SaveQuery, Some("Studio")),
            KeyBinding::new(&format!("{modifier}-o"), OpenQuery, Some("Studio")),
            KeyBinding::new(&format!("{modifier}-shift-f"), FormatQuery, Some("Studio")),
        ]);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputTab {
    Results,
    Messages,
    History,
    Schema,
    Saved,
}
impl OutputTab {
    fn index(self) -> usize {
        match self {
            Self::Results => 0,
            Self::Messages => 1,
            Self::History => 2,
            Self::Schema => 3,
            Self::Saved => 4,
        }
    }
}

struct QueryTab {
    id: usize,
    title: String,
    editor: Entity<EditorState>,
    editor_source: String,
    results: Entity<TableState<QueryResults>>,
    outputs: Vec<QueryResult>,
    result_index: usize,
    message: String,
    is_error: bool,
    dirty: bool,
    revision: usize,
    executed_revision: Option<usize>,
    source_object: Option<DbObject>,
    browse: Option<TableBrowseState>,
    saved_id: Option<String>,
    connection_id: Option<String>,
    elapsed_ms: u64,
    cancel: Option<CancelToken>,
    _task: Option<Task<()>>,
    _changes: Subscription,
}

#[derive(Clone)]
struct TableBrowseState {
    engine: crate::database::Engine,
    request: crate::table_browse::BrowseRequest,
    columns: Vec<ColumnDetails>,
}

struct LiveConnection {
    config: ConnectionConfig,
    session: DatabaseSession,
    catalog: Vec<DbObject>,
}

pub struct Studio {
    focus: FocusHandle,
    tabs: Vec<QueryTab>,
    active: usize,
    next_id: usize,
    output_tab: OutputTab,
    show_sidebar: bool,
    show_results: bool,
    filter: Entity<InputState>,
    result_filter: Entity<InputState>,
    saved_filter: Entity<InputState>,
    history_filter: Entity<InputState>,
    objects: Entity<TreeState>,
    collapsed_objects: std::collections::HashSet<String>,
    connections: Vec<LiveConnection>,
    active_connection: Option<String>,
    connecting: bool,
    schema: Vec<ColumnDetails>,
    schema_indexes: Vec<IndexDetails>,
    schema_foreign_keys: Vec<ForeignKeyDetails>,
    schema_title: String,
    notice: String,
    storage_error: Option<String>,
    store: WorkspaceStore,
    credentials: std::sync::Arc<dyn crate::credentials::PasswordStore>,
    workspace: WorkspaceData,
    update_state: updates::UpdateState,
    installing_update: bool,
    _update_task: Option<Task<()>>,
    _update_timer: Option<Task<()>>,
    _connection_task: Option<Task<()>>,
    _catalog_task: Option<Task<()>>,
    _file_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Studio {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        match WorkspaceStore::open_default() {
            Ok((store, data)) => Self::with_store(store, data, window, cx),
            Err(error) => {
                let mut studio = Self::with_store(
                    WorkspaceStore::in_memory(),
                    WorkspaceData::default(),
                    window,
                    cx,
                );
                studio.storage_error = Some(format!(
                    "Workspace could not be loaded: {error}. This session will not be saved."
                ));
                studio
            }
        }
    }

    fn with_store(
        store: WorkspaceStore,
        workspace: WorkspaceData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::studio_theme::apply(
            if workspace.preferences.dark_mode {
                gpui_kit::component::ThemeMode::Dark
            } else {
                gpui_kit::component::ThemeMode::Light
            },
            Some(window),
            cx,
        );
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables and views…"));
        let result_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter loaded rows…"));
        let objects = cx.new(|cx| TreeState::new(cx));
        let saved_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search saved queries"));
        let history_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search query history"));
        let mut studio = Self {
            focus: cx.focus_handle(),
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            output_tab: OutputTab::Results,
            show_sidebar: workspace.preferences.show_sidebar,
            show_results: workspace.preferences.show_results,
            filter: filter.clone(),
            result_filter: result_filter.clone(),
            saved_filter: saved_filter.clone(),
            history_filter: history_filter.clone(),
            objects: objects.clone(),
            collapsed_objects: std::collections::HashSet::new(),
            connections: Vec::new(),
            active_connection: None,
            connecting: false,
            schema: Vec::new(),
            schema_indexes: Vec::new(),
            schema_foreign_keys: Vec::new(),
            schema_title: String::new(),
            notice: String::new(),
            storage_error: None,
            store,
            credentials: crate::credentials::native_store(),
            workspace,
            update_state: updates::UpdateState::Idle,
            installing_update: false,
            _update_task: None,
            _update_timer: None,
            _connection_task: None,
            _catalog_task: None,
            _file_task: None,
            _subscriptions: Vec::new(),
        };
        studio
            ._subscriptions
            .push(cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.rebuild_tree(cx);
                }
            }));
        studio._subscriptions.push(cx.subscribe(
            &result_filter,
            |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    if let Some(tab) = this.tabs.get_mut(this.active) {
                        tab.results.update(cx, |state, cx| {
                            state.delegate_mut().filter(&text);
                            state.refresh(cx);
                        });
                    }
                }
            },
        ));
        studio
            ._subscriptions
            .push(cx.observe(&objects, |_, _, cx| cx.notify()));
        studio
            ._subscriptions
            .push(
                cx.subscribe(&objects, |this, _, event: &TreeEvent, _| match event {
                    TreeEvent::Expanded(id) => {
                        this.collapsed_objects.remove(id.as_str());
                    }
                    TreeEvent::Collapsed(id) => {
                        this.collapsed_objects.insert(id.to_string());
                    }
                }),
            );
        for input in [&saved_filter, &history_filter] {
            studio
                ._subscriptions
                .push(cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }));
        }
        studio.add_query(
            "Query 1".into(),
            "-- Connect a database, then run SQL with Ctrl+Enter.\nSELECT 1 AS result;\n".into(),
            window,
            cx,
        );
        studio.start_update_timer(cx);
        studio
    }

    fn connection(&self) -> Option<&LiveConnection> {
        self.active_connection
            .as_ref()
            .and_then(|id| self.connections.iter().find(|c| &c.config.id == id))
    }

    fn any_running(&self) -> bool {
        self.tabs.iter().any(|tab| tab.cancel.is_some())
    }

    fn clear_schema(&mut self) {
        self.schema.clear();
        self.schema_indexes.clear();
        self.schema_foreign_keys.clear();
        self.schema_title.clear();
    }

    fn persist(&mut self) -> bool {
        if self.storage_error.is_some() {
            self.notice = "Workspace unavailable. Save SQL to a file to keep your work.".into();
            return false;
        }
        self.workspace.preferences.show_sidebar = self.show_sidebar;
        self.workspace.preferences.show_results = self.show_results;
        if let Err(error) = self.store.save(&self.workspace) {
            self.notice = format!("Workspace save failed: {error}");
            return false;
        }
        true
    }

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter.read(cx).value().to_lowercase();
        let items = self
            .connection()
            .map(|connection| {
                navigator::catalog_tree(
                    &connection.config,
                    &connection.catalog,
                    &filter,
                    &self.collapsed_objects,
                )
            })
            .unwrap_or_default();
        self.objects
            .update(cx, |state, cx| state.set_items(items, cx));
        self.refresh_completions(cx);
        cx.notify();
    }

    fn refresh_completions(&self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            let id = tab
                .connection_id
                .as_ref()
                .or(self.active_connection.as_ref());
            let connection = id.and_then(|id| {
                self.connections
                    .iter()
                    .find(|connection| &connection.config.id == id)
            });
            let provider = if let Some(connection) = connection {
                crate::sql_completion::SqlCompletion::new(
                    connection.config.engine,
                    &connection.catalog,
                    if Some(&connection.config.id) == self.active_connection.as_ref() {
                        &self.schema
                    } else {
                        &[]
                    },
                )
            } else {
                crate::sql_completion::SqlCompletion::new(crate::database::Engine::SQLite, &[], &[])
            };
            tab.editor.update(cx, |state, _| {
                state.lsp_mut().completion_provider = Some(std::rc::Rc::new(provider))
            });
        }
    }

    fn selected_object(&self, cx: &App) -> Option<DbObject> {
        let id = self.objects.read(cx).selected_entry()?.item().id.as_str();
        let index = id.strip_prefix("object-")?.parse::<usize>().ok()?;
        self.connection()?.catalog.get(index).cloned()
    }

    fn add_query(
        &mut self,
        title: String,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.len() >= MAX_TABS {
            self.notice = "The maximum of 32 open queries has been reached.".into();
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let editor_source = source.clone();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("sql")
                .line_number(true)
                .folding(true)
                .default_value(source)
        });
        let results = cx.new(|cx| {
            TableState::new(QueryResults::empty(), window, cx)
                .cell_selectable(true)
                .col_movable(false)
        });
        let changes = cx.subscribe(&editor, move |this, editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                    let source = editor.read(cx).value().to_string();
                    if source == tab.editor_source {
                        return;
                    }
                    tab.editor_source = source;
                    tab.dirty = true;
                    tab.revision += 1;
                    tab.source_object = None;
                    tab.browse = None;
                    // Keep the last executed result available, with an explicit stale marker.
                    if !tab.outputs.is_empty() {
                        tab.message =
                            "Query edited since the displayed result was produced.".into();
                    }
                }
                cx.notify();
            }
        });
        self.tabs.push(QueryTab {
            id,
            title,
            editor: editor.clone(),
            editor_source,
            results,
            outputs: Vec::new(),
            result_index: 0,
            message: "Ready. Results are bounded by the selected row limit.".into(),
            is_error: false,
            dirty: false,
            revision: 0,
            executed_revision: None,
            source_object: None,
            browse: None,
            saved_id: None,
            connection_id: self.active_connection.clone(),
            elapsed_ms: 0,
            cancel: None,
            _task: None,
            _changes: changes,
        });
        self.active = self.tabs.len() - 1;
        self.refresh_completions(cx);
        editor.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn new_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_query(
            format!("Query {}", self.next_id),
            "SELECT 1 AS result;\n".into(),
            window,
            cx,
        );
    }

    fn open_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() >= MAX_TABS {
            self.notice = "Close a query before opening another table.".into();
            cx.notify();
            return;
        }
        if let (Some(object), Some(connection)) = (self.selected_object(cx), self.connection()) {
            let sql = format!(
                "SELECT * FROM {} LIMIT {};",
                connection.session.quote_object(&object),
                self.workspace.preferences.row_limit
            );
            self.add_query(format!("{}.sql", object.name), sql, window, cx);
            self.tabs[self.active].source_object = Some(object);
        }
    }

    fn select_result(&mut self, index: usize, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        if let Some(result) = tab.outputs.get(index) {
            tab.result_index = index;
            let mut delegate = QueryResults::from_result(result);
            delegate.filter(self.result_filter.read(cx).value().as_str());
            tab.results.update(cx, |state, cx| {
                *state.delegate_mut() = delegate;
                state.refresh(cx);
            });
        }
        cx.notify();
    }

    fn close_query(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1
            || self
                .tabs
                .iter()
                .find(|t| t.id == id)
                .is_some_and(|t| t.cancel.is_some())
        {
            return;
        }
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let view = view.clone();
            dialogs::frame(
                dialog,
                "Query workspace",
                "Close this query?",
                "Keep a saved copy before closing this draft.",
                460.,
                cx,
            )
            .child("Save this draft to the workspace or a SQL file before closing to keep it.")
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-close")
                            .label("Keep query")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("confirm-close")
                            .danger()
                            .label("Close query")
                            .on_click(move |_, window, cx| {
                                if let Some(view) = view.upgrade() {
                                    view.update(cx, |this, cx| {
                                        if let Some(index) = this
                                            .tabs
                                            .iter()
                                            .position(|t| t.id == id && t.cancel.is_none())
                                        {
                                            this.tabs.remove(index);
                                            if this.active >= index {
                                                this.active = this.active.saturating_sub(1);
                                            }
                                            this.tabs[this.active]
                                                .editor
                                                .update(cx, |state, cx| state.focus(window, cx));
                                            cx.notify();
                                        }
                                    });
                                }
                                window.close_dialog(cx);
                            }),
                    ),
            )
        });
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        // Closing the workbench requests cancellation before its task handles
        // and database sessions are released.
        for tab in &self.tabs {
            if let Some(cancel) = &tab.cancel {
                cancel.cancel();
            }
        }
    }
}
