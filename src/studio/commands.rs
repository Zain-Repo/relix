//! Searchable workbench commands and query navigation using the native Kit list.

use super::*;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{
    ActiveTheme, IndexPath,
    list::{List, ListDelegate, ListItem, ListState},
    v_flex,
};
use gpui_kit::{WeakEntity, div, px};

#[derive(Clone)]
enum PaletteAction {
    NewQuery,
    RunQuery,
    RunStatement,
    StopQuery,
    FormatQuery,
    SaveQuery,
    OpenSql,
    ToggleExplorer,
    ToggleResults,
    Connect,
    Scratch,
    RefreshSchema,
    OpenTab(usize),
    OpenSaved(String),
}

struct PaletteEntry {
    label: String,
    detail: String,
    shortcut: &'static str,
    search: String,
    action: PaletteAction,
}

impl PaletteEntry {
    fn new(
        label: impl Into<String>,
        detail: impl Into<String>,
        shortcut: &'static str,
        action: PaletteAction,
    ) -> Self {
        let label = label.into();
        let detail = detail.into();
        let search = format!("{label} {detail}").to_lowercase();
        let shortcut = if shortcut.is_empty() {
            match (&action, cfg!(target_os = "macos")) {
                (PaletteAction::NewQuery, false) => "Ctrl+N",
                (PaletteAction::NewQuery, true) => "Cmd+N",
                (PaletteAction::RunQuery, false) => "Ctrl+Enter",
                (PaletteAction::RunQuery, true) => "Cmd+Enter",
                (PaletteAction::RunStatement, false) => "Ctrl+Shift+Enter",
                (PaletteAction::RunStatement, true) => "Cmd+Shift+Enter",
                (PaletteAction::FormatQuery, false) => "Ctrl+Shift+F",
                (PaletteAction::FormatQuery, true) => "Cmd+Shift+F",
                (PaletteAction::SaveQuery, false) => "Ctrl+S",
                (PaletteAction::SaveQuery, true) => "Cmd+S",
                (PaletteAction::OpenSql, false) => "Ctrl+O",
                (PaletteAction::OpenSql, true) => "Cmd+O",
                (PaletteAction::ToggleExplorer, false) => "Ctrl+B",
                (PaletteAction::ToggleExplorer, true) => "Cmd+B",
                (PaletteAction::ToggleResults, false) => "Ctrl+J",
                (PaletteAction::ToggleResults, true) => "Cmd+J",
                _ => "",
            }
        } else {
            shortcut
        };
        Self {
            label,
            detail,
            shortcut,
            search,
            action,
        }
    }
}

struct CommandPalette {
    studio: WeakEntity<Studio>,
    entries: Vec<PaletteEntry>,
    matches: Vec<usize>,
    selected: Option<IndexPath>,
}

impl ListDelegate for CommandPalette {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.matches.len()
    }

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        let terms = query.to_lowercase();
        self.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                terms
                    .split_whitespace()
                    .all(|term| entry.search.contains(term))
            })
            .map(|(index, _)| index)
            .collect();
        self.selected = (!self.matches.is_empty()).then(IndexPath::default);
        cx.notify();
        // The Kit updates its own selection after calling the delegate. Set the
        // filtered selection once that update finishes, including empty -> full searches.
        let selected = self.selected;
        cx.spawn_in(window, async move |list, cx| {
            let _ = list.update_in(cx, |list, window, cx| {
                list.set_selected_index(selected, window, cx);
                cx.notify();
            });
        })
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let entry_index = *self.matches.get(index.row)?;
        let entry = self.entries.get(entry_index)?;
        let studio = self.studio.upgrade()?;
        let unavailable = studio.read(cx).command_unavailable(&entry.action, cx);
        let detail = unavailable.unwrap_or(&entry.detail).to_owned();
        Some(
            ListItem::new(("workbench-command", entry_index))
                .h(px(60.))
                .px_3()
                .disabled(unavailable.is_some())
                .accessibility_label(format!("{}. {detail}", entry.label))
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .id(("command-label", entry_index))
                                        .text_sm()
                                        .truncate()
                                        .child(entry.label.clone())
                                        .test_support(),
                                )
                                .child(
                                    div()
                                        .id(("command-detail", entry_index))
                                        .text_xs()
                                        .truncate()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(detail)
                                        .test_support(),
                                ),
                        )
                        .when(!entry.shortcut.is_empty(), |row| {
                            row.child(
                                div()
                                    .id(("command-shortcut", entry_index))
                                    .flex_shrink_0()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(entry.shortcut)
                                    .test_support(),
                            )
                        }),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_4()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("No matching commands or queries.")
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(entry) = self
            .selected
            .and_then(|index| self.matches.get(index.row))
            .and_then(|index| self.entries.get(*index))
        else {
            return;
        };
        let Some(studio) = self.studio.upgrade() else {
            return;
        };
        // Keyboard confirmation must enforce the same availability as mouse input.
        if studio
            .read(cx)
            .command_unavailable(&entry.action, cx)
            .is_some()
        {
            return;
        }
        let action = entry.action.clone();
        window.close_dialog(cx);
        studio.update(cx, |studio, cx| studio.execute_command(action, window, cx));
    }

    fn cancel(&mut self, _: &mut Window, _: &mut Context<ListState<Self>>) {
        // Kit List propagates Escape to its parent dialog, which owns dismissal
        // and restores the previous focused editor or toolbar control.
    }
}

impl Studio {
    /// Open the command palette without taking ownership of query drafts.
    pub(super) fn open_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        let mut entries = vec![
            PaletteEntry::new(
                "New query",
                "Create a SQL draft",
                "",
                PaletteAction::NewQuery,
            ),
            PaletteEntry::new(
                "Run query",
                "Run selected SQL, or the full editor",
                "",
                PaletteAction::RunQuery,
            ),
            PaletteEntry::new(
                "Run statement at cursor",
                "Run the current SQL statement",
                "",
                PaletteAction::RunStatement,
            ),
            PaletteEntry::new(
                "Stop query",
                "Request cancellation of the active query",
                "",
                PaletteAction::StopQuery,
            ),
            PaletteEntry::new(
                "Format SQL",
                "Format the current query using its database dialect",
                "",
                PaletteAction::FormatQuery,
            ),
            PaletteEntry::new(
                "Save query",
                "Save the active query to this workspace",
                "",
                PaletteAction::SaveQuery,
            ),
            PaletteEntry::new(
                "Open SQL file",
                "Load a SQL file into a new query tab",
                "",
                PaletteAction::OpenSql,
            ),
            PaletteEntry::new(
                "Toggle explorer",
                "Show or hide the database sidebar",
                "",
                PaletteAction::ToggleExplorer,
            ),
            PaletteEntry::new(
                "Toggle results",
                "Show or hide the output panel",
                "",
                PaletteAction::ToggleResults,
            ),
            PaletteEntry::new(
                "Connect database",
                "Configure or connect a database",
                "",
                PaletteAction::Connect,
            ),
            PaletteEntry::new(
                "Create SQLite scratch",
                "Open a temporary in-memory database",
                "",
                PaletteAction::Scratch,
            ),
            PaletteEntry::new(
                "Refresh schema",
                "Reload tables and views for the active connection",
                "",
                PaletteAction::RefreshSchema,
            ),
        ];
        entries.extend(self.tabs.iter().take(MAX_TABS).map(|tab| {
            PaletteEntry::new(
                format!("Open query: {}", tab.title),
                self.command_connection_name(tab.connection_id.as_deref()),
                "",
                PaletteAction::OpenTab(tab.id),
            )
        }));
        entries.extend(
            self.workspace
                .queries
                .iter()
                .take(crate::workspace::MAX_SAVED_QUERIES)
                .map(|query| {
                    PaletteEntry::new(
                        format!("Saved query: {}", query.title),
                        self.command_connection_name(query.connection_id.as_deref()),
                        "",
                        PaletteAction::OpenSaved(query.id.clone()),
                    )
                }),
        );
        let matches = (0..entries.len()).collect();
        let studio = cx.entity().downgrade();
        let list = cx.new(|cx| {
            let mut list = ListState::new(
                CommandPalette {
                    studio,
                    entries,
                    matches,
                    selected: Some(IndexPath::default()),
                },
                window,
                cx,
            )
            .searchable(true);
            list.set_selected_index(Some(IndexPath::default()), window, cx);
            list
        });
        let focus = list.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            dialogs::frame(
                dialog,
                "Quick access",
                "Commands and queries",
                "Find an action or jump to a query. Use ↑ ↓ to navigate and Enter to choose.",
                600.,
                cx,
            )
            .child(
                List::new(&list)
                    .search_placeholder("Search commands, open tabs, saved queries…")
                    .h(px(360.)),
            )
            .footer(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "↑↓ Navigate · Enter Select · Esc Close · {modifier}+K Open"
                    )),
            )
        });
        focus.update(cx, |list, cx| list.focus(window, cx));
    }

    fn command_connection_name(&self, id: Option<&str>) -> String {
        id.and_then(|id| {
            self.connections
                .iter()
                .find(|connection| connection.config.id == id)
                .map(|connection| connection.config.name.as_str())
                .or_else(|| {
                    self.workspace
                        .connections
                        .iter()
                        .find(|connection| connection.id == id)
                        .map(|connection| connection.name.as_str())
                })
        })
        .unwrap_or("Session draft")
        .to_owned()
    }

    fn command_unavailable(&self, action: &PaletteAction, cx: &App) -> Option<&'static str> {
        let tab = self.tabs.get(self.active);
        let busy = self.any_running() || self.connecting;
        match action {
            PaletteAction::NewQuery | PaletteAction::OpenSql if self.tabs.len() >= MAX_TABS => {
                Some("Close a query first; 32 tabs are already open")
            }
            PaletteAction::RunQuery | PaletteAction::RunStatement if busy => {
                Some("Wait for the current database operation")
            }
            PaletteAction::RunQuery | PaletteAction::RunStatement
                if self.connection().is_none() =>
            {
                Some("Connect a database or create a scratch database first")
            }
            PaletteAction::RunQuery | PaletteAction::RunStatement
                if tab.is_some_and(|tab| {
                    tab.connection_id
                        .as_ref()
                        .is_some_and(|id| Some(id) != self.active_connection.as_ref())
                }) =>
            {
                Some("Select this query's original connection before running")
            }
            PaletteAction::RunQuery | PaletteAction::RunStatement
                if tab.is_none_or(|tab| tab.editor.read(cx).value().trim().is_empty()) =>
            {
                Some("Enter SQL in the active query")
            }
            PaletteAction::StopQuery if tab.is_none_or(|tab| tab.cancel.is_none()) => {
                Some("The active query is not running")
            }
            PaletteAction::FormatQuery if tab.is_none_or(|tab| tab.cancel.is_some()) => {
                Some("Wait for the active query to finish")
            }
            PaletteAction::SaveQuery if self.storage_error.is_some() => {
                Some("Workspace storage is unavailable; save SQL to a file")
            }
            PaletteAction::SaveQuery
                if tab.is_some_and(|tab| tab.saved_id.is_none())
                    && self.workspace.queries.len() >= crate::workspace::MAX_SAVED_QUERIES =>
            {
                Some("Remove a saved query before adding another")
            }
            PaletteAction::Connect | PaletteAction::Scratch | PaletteAction::RefreshSchema
                if busy =>
            {
                Some("Wait for the current database operation")
            }
            PaletteAction::RefreshSchema if self.connection().is_none() => {
                Some("Connect a database first")
            }
            PaletteAction::OpenTab(id) if !self.tabs.iter().any(|tab| tab.id == *id) => {
                Some("This query has been closed")
            }
            PaletteAction::OpenSaved(id)
                if !self.workspace.queries.iter().any(|query| query.id == *id) =>
            {
                Some("This saved query has been removed")
            }
            PaletteAction::OpenSaved(id)
                if self.tabs.len() >= MAX_TABS
                    && !self
                        .tabs
                        .iter()
                        .any(|tab| tab.saved_id.as_ref() == Some(id)) =>
            {
                Some("Close a query first; 32 tabs are already open")
            }
            _ => None,
        }
    }

    fn execute_command(
        &mut self,
        action: PaletteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(reason) = self.command_unavailable(&action, cx) {
            self.notice = reason.into();
            cx.notify();
            return;
        }
        match action {
            PaletteAction::NewQuery => self.new_query(window, cx),
            PaletteAction::RunQuery => self.run_query(window, cx),
            PaletteAction::RunStatement => self.run_statement(window, cx),
            PaletteAction::StopQuery => self.stop_query(cx),
            PaletteAction::FormatQuery => self.format_query(window, cx),
            PaletteAction::SaveQuery => self.save_query(cx),
            PaletteAction::OpenSql => self.open_sql_file(false, window, cx),
            PaletteAction::ToggleExplorer => {
                self.show_sidebar = !self.show_sidebar;
                self.persist();
            }
            PaletteAction::ToggleResults => {
                self.show_results = !self.show_results;
                self.persist();
            }
            PaletteAction::Connect => self.connection_dialog(None, window, cx),
            PaletteAction::Scratch => self.scratch(cx),
            PaletteAction::RefreshSchema => self.refresh_catalog(cx),
            PaletteAction::OpenTab(id) => {
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    self.active = index;
                    self.tabs[index]
                        .editor
                        .update(cx, |editor, cx| editor.focus(window, cx));
                }
            }
            PaletteAction::OpenSaved(id) => {
                if let Some(index) = self
                    .tabs
                    .iter()
                    .position(|tab| tab.saved_id.as_ref() == Some(&id))
                {
                    self.active = index;
                    self.tabs[index]
                        .editor
                        .update(cx, |editor, cx| editor.focus(window, cx));
                } else if let Some(query) = self
                    .workspace
                    .queries
                    .iter()
                    .find(|query| query.id == id)
                    .cloned()
                {
                    self.add_query(query.title, query.sql, window, cx);
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.saved_id = Some(query.id);
                        tab.connection_id = query.connection_id;
                        tab.dirty = false;
                    }
                    self.refresh_completions(cx);
                }
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::SavedQuery;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AnyWindowHandle, TestAppContext, WindowOptions};

    #[gpui_kit::test]
    fn palette_labels_details_and_shortcuts_fit_their_rows_in_both_themes(cx: &mut TestAppContext) {
        let (handle, studio) = super::super::tests::open_studio(cx, 900., 620.);
        for mode in [
            gpui_kit::component::ThemeMode::Light,
            gpui_kit::component::ThemeMode::Dark,
        ] {
            cx.update_window(handle, |_, window, cx| {
                crate::studio_theme::apply(mode, Some(window), cx);
                studio.update(cx, |view, cx| view.open_commands(window, cx));
            })
            .expect("Open palette");
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                for index in 0..3usize {
                    let row = window.find(("workbench-command", index)).bounds();
                    let label = window.find(("command-label", index)).bounds();
                    let detail = window.find(("command-detail", index)).bounds();
                    let shortcut = window.find(("command-shortcut", index)).bounds();
                    assert!(
                        label.bottom() <= detail.top(),
                        "Label and detail must not overlap"
                    );
                    assert!(detail.bottom() <= row.bottom() && label.top() >= row.top());
                    assert!(detail.right() <= shortcut.left() && shortcut.right() <= row.right());
                    assert!(shortcut.top() >= row.top() && shortcut.bottom() <= row.bottom());
                }
                window.press("enter", cx);
            })
            .expect("Palette row geometry and keyboard activation");
            cx.run_until_parked();
        }
        cx.update(|cx| assert_eq!(studio.read(cx).tabs.len(), 3));
    }

    fn open_studio(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Studio>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    Studio::with_store(
                        WorkspaceStore::in_memory(),
                        WorkspaceData::default(),
                        window,
                        cx,
                    )
                })
            })
            .expect("Command palette test window must open")
        })
    }

    #[gpui_kit::test]
    fn saved_commands_preserve_ownership_and_never_replace_a_draft_at_capacity(
        cx: &mut TestAppContext,
    ) {
        let (handle, studio) = open_studio(cx);
        cx.update_window(handle, |_, window, cx| {
            studio.update(cx, |studio, cx| {
                studio.active_connection = Some("different-server".into());
                studio.workspace.queries.push(SavedQuery {
                    id: "saved".into(),
                    title: "Original query".into(),
                    sql: "SELECT 7;".into(),
                    connection_id: Some("original-server".into()),
                });
                studio.execute_command(PaletteAction::OpenSaved("saved".into()), window, cx);
                let saved_tab = studio.tabs[studio.active].id;
                assert_eq!(
                    studio.tabs[studio.active].connection_id.as_deref(),
                    Some("original-server")
                );
                while studio.tabs.len() < MAX_TABS {
                    studio.new_query(window, cx);
                }
                studio.execute_command(PaletteAction::OpenSaved("saved".into()), window, cx);
                assert_eq!(studio.tabs[studio.active].id, saved_tab);
                studio.workspace.queries.push(SavedQuery {
                    id: "other".into(),
                    title: "Another query".into(),
                    sql: "SELECT 42;".into(),
                    connection_id: None,
                });
                studio.execute_command(PaletteAction::OpenSaved("other".into()), window, cx);
                assert_eq!(studio.tabs.len(), MAX_TABS);
                assert_eq!(studio.tabs[studio.active].id, saved_tab);
                assert_eq!(
                    studio.tabs[studio.active].editor.read(cx).value().as_str(),
                    "SELECT 7;"
                );
            });
        })
        .expect("Saved query commands must preserve existing drafts");
    }
}
