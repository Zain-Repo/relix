use super::*;
use crate::database::Engine;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};

#[gpui_kit::test]
fn result_tools_remain_reachable_in_compact_and_wide_workspaces(cx: &mut TestAppContext) {
    for width in [900., 1440.] {
        let (handle, _) = open_studio(cx, width, 820.);
        for mode in [
            gpui_kit::component::ThemeMode::Light,
            gpui_kit::component::ThemeMode::Dark,
        ] {
            cx.update_window(handle, |_, window, cx| {
                crate::studio_theme::apply(mode, Some(window), cx);
                window.render_frame(cx);
                let filter = window.find("result-filter-control").bounds();
                let export = window.find("export-results").bounds();
                for (id, bounds) in [("filter", filter), ("export", export)] {
                    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                    assert!(
                        bounds.left() >= px(0.)
                            && bounds.right() <= px(width)
                            && bounds.top() >= px(0.)
                            && bounds.bottom() <= px(820.),
                        "{id} must fit {width}px in {mode:?}: {bounds:?}"
                    );
                }
                if width >= 1280. {
                    assert!(export.right() <= filter.left());
                    assert!(export.top() < filter.bottom() && filter.top() < export.bottom());
                } else {
                    assert!(filter.bottom() <= export.top());
                }
            })
            .expect("Responsive result tools must render");
        }
    }
}

#[gpui_kit::test]
fn long_draft_target_and_saved_profiles_fit_minimum_workspace(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let original = ConnectionConfig {
                id: "original-test".into(),
                name: "Original connection with a deliberately long descriptive name".into(),
                ..Default::default()
            };
            view.tabs[view.active].connection_id = Some(original.id.clone());
            view.workspace.connections = vec![original];
            for engine in [Engine::DuckDB, Engine::Postgres, Engine::MySql] {
                view.workspace.connections.push(ConnectionConfig {
                    id: format!("saved-{}", engine.name()),
                    engine,
                    ..Default::default()
                });
            }
            let config = ConnectionConfig {
                id: "scratch-test".into(),
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            };
            let session = DatabaseSession::connect(&config).expect("Scratch fixture");
            view.active_connection = Some(config.id.clone());
            view.connections.push(LiveConnection {
                config,
                session,
                catalog: Vec::new(),
            });
            view.rebuild_tree(cx);
        });
        window.render_frame(cx);
        for id in [
            "draft-target-mismatch",
            "retarget-query",
            "run-query",
            "open-table",
        ] {
            let bounds = window.find(id).bounds();
            assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
            assert!(
                bounds.left() >= px(0.)
                    && bounds.right() <= px(900.)
                    && bounds.top() >= px(0.)
                    && bounds.bottom() <= px(620.),
                "{id} must fit with saved profiles and a mismatched target: {bounds:?}"
            );
        }
        assert!(window.find("database-navigator").bounds().size.height > px(0.));
    })
    .expect("Minimum workspace target context");
}

#[gpui_kit::test]
fn navigator_folder_selection_cannot_open_an_object_and_filtered_leaf_keeps_identity(
    cx: &mut TestAppContext,
) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let config = ConnectionConfig { database: ":memory:".into(), read_only: false, ..Default::default() };
            let session = DatabaseSession::connect(&config).expect("Scratch database");
            session.execute("CREATE TABLE zebra(id INTEGER); CREATE TABLE accounts(id INTEGER); CREATE VIEW summary AS SELECT * FROM accounts;", 100, &CancelToken::new()).expect("Catalog fixture");
            let catalog = session.catalog().expect("Catalog");
            view.active_connection = Some(config.id.clone());
            view.connections.push(LiveConnection { config, session, catalog });
            view.rebuild_tree(cx);
            view.objects.update(cx, |state, cx| state.set_selected_index(Some(0), cx));
            assert!(view.selected_object(cx).is_none());
            let tabs = view.tabs.len();
            view.open_table(window, cx);
            assert_eq!(view.tabs.len(), tabs);
            view.filter.update(cx, |state, cx| state.set_value("accounts", window, cx));
        });
    }).expect("Navigator setup");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let index = view
                .connection()
                .expect("Connection")
                .catalog
                .iter()
                .position(|object| object.name == "accounts")
                .expect("Accounts");
            let item =
                gpui_kit::component::tree::TreeItem::new(format!("object-{index}"), "accounts");
            view.objects
                .update(cx, |state, cx| state.set_selected_item(Some(&item), cx));
            assert_eq!(
                view.selected_object(cx).expect("Selected leaf").name,
                "accounts"
            );
            view.open_table(window, cx);
            assert!(
                view.tabs[view.active]
                    .editor
                    .read(cx)
                    .value()
                    .contains("\"accounts\"")
            );
        });
        window.render_frame(cx);
        let bounds = window.find("database-navigator").bounds();
        assert!(bounds.size.height > px(100.) && bounds.right() <= px(900.));
    })
    .expect("Filtered navigator identity");
}

#[gpui_kit::test]
fn connection_dialog_footers_fit_minimum_window_in_both_themes(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    for mode in [
        gpui_kit::component::ThemeMode::Light,
        gpui_kit::component::ThemeMode::Dark,
    ] {
        cx.update_window(handle, |_, window, cx| {
            crate::studio_theme::apply(mode, Some(window), cx);
            studio.update(cx, |view, cx| view.connection_dialog(None, window, cx));
        })
        .expect("Open connection dialog");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for id in ["cancel-connect", "choose-engine-next"] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.size.height > px(0.) && bounds.bottom() <= px(620.),
                    "{id}: {bounds:?}"
                );
            }
            for index in 0..4usize {
                let bounds = window.find(("engine", index)).bounds();
                assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                assert!(
                    bounds.left() >= px(0.)
                        && bounds.right() <= px(900.)
                        && bounds.bottom() <= px(620.)
                );
            }
            window.click(("engine", 2usize), cx);
            window.click("choose-engine-next", cx);
            window.render_frame(cx);
            for id in [
                "back-to-engines",
                "cancel-connect",
                "test-connection",
                "connect-database",
            ] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.size.height > px(0.) && bounds.bottom() <= px(620.),
                    "Server {id}: {bounds:?}"
                );
            }
            window.click("back-to-engines", cx);
            window.render_frame(cx);
            window.click(("engine", 0usize), cx);
            window.click("choose-engine-next", cx);
            window.render_frame(cx);
            for id in ["cancel-connect", "test-connection", "connect-database"] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.size.height > px(0.) && bounds.bottom() <= px(620.),
                    "Local {id}: {bounds:?}"
                );
            }
            window.press("escape", cx);
        })
        .expect("Connection dialog layout");
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn files_menu_routes_keyboard_save_to_workspace(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("query-files", cx);
    })
    .expect("Open Files menu");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("down", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .expect("Choose save action");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(studio.read(cx).workspace.queries.len(), 1));
}

#[gpui_kit::test]
fn rename_synchronizes_saved_tab_titles_and_keeps_invalid_dialog_open(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            view.tabs[0].title = "New name".into();
            view.save_query(cx);
            let saved_id = view.tabs[0].saved_id.clone();
            view.add_query("Old name".into(), "SELECT 2;".into(), window, cx);
            view.tabs[1].saved_id = saved_id;
            view.active = 0;
            view.rename_query(window, cx);
        });
    })
    .expect("Open rename dialog");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("confirm-rename", cx);
        studio.update(cx, |view, cx| {
            assert_eq!(view.tabs[1].title, "New name");
            view.active = 1;
            view.save_query(cx);
            assert_eq!(view.workspace.queries[0].title, "New name");
            view.tabs[1].title.clear();
            view.rename_query(window, cx);
        });
    })
    .expect("Rename captured query");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("confirm-rename", cx);
        window.render_frame(cx);
        assert!(window.find("cancel-rename").bounds().size.width > px(0.));
        window.click("cancel-rename", cx);
    })
    .expect("Saved title synchronization");
}

#[gpui_kit::test]
fn insert_form_prepares_sql_without_writing_and_rejects_stale_source(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let config = ConnectionConfig {
                database: ":memory:".into(),
                read_only: false,
                ..Default::default()
            };
            let session = DatabaseSession::connect(&config).expect("Scratch session");
            session
                .execute(
                    "CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT DEFAULT 'default');",
                    100,
                    &CancelToken::new(),
                )
                .expect("Fixture table");
            view.active_connection = Some(config.id.clone());
            view.connections.push(LiveConnection {
                config,
                session,
                catalog: Vec::new(),
            });
            view.add_query(
                "Table".into(),
                "SELECT * FROM items LIMIT 100;".into(),
                window,
                cx,
            );
            view.tabs[view.active].source_object = Some(DbObject {
                schema: "main".into(),
                name: "items".into(),
                kind: crate::database::ObjectKind::Table,
            });
            view.run_query(window, cx);
        });
    })
    .expect("Table query");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| view.insert_row(window, cx));
    })
    .expect("Insert form");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("prepare-row-operation", cx);
        let view = studio.read(cx);
        assert_eq!(view.tabs.len(), 3);
        assert!(
            view.tabs[view.active]
                .editor
                .read(cx)
                .value()
                .contains("DEFAULT VALUES")
        );
        let result = view
            .connection()
            .expect("Connection")
            .session
            .execute("SELECT COUNT(*) FROM items;", 10, &CancelToken::new())
            .expect("Read table");
        assert_eq!(result.results[0].rows[0][0].as_deref(), Some("0"));
        studio.update(cx, |view, cx| {
            view.active = 1;
            view.tabs[1].revision += 1;
            view.insert_row(window, cx);
            assert!(view.notice.contains("rerun"));
            assert_eq!(view.tabs.len(), 3);
        });
    })
    .expect("Preview does not write");
}

#[gpui_kit::test]
fn palette_keyboard_confirmation_and_escape_restore_workspace(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        studio.update(cx, |view, cx| {
            view.tabs[view.active]
                .editor
                .update(cx, |state, cx| state.focus(window, cx))
        });
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .expect("Palette confirmation");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(studio.read(cx).tabs.len(), 2);
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
        window.press("ctrl-n", cx);
        assert_eq!(studio.read(cx).tabs.len(), 3);
    })
    .expect("Palette cancellation restores focus");
}

#[gpui_kit::test]
fn duplicate_is_an_independent_draft_with_original_connection(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            view.tabs[0].connection_id = Some("original".into());
            view.tabs[0].saved_id = Some("saved".into());
            view.active_connection = Some("other".into());
            view.duplicate_query(window, cx);
            let duplicate = &view.tabs[1];
            assert_eq!(duplicate.connection_id.as_deref(), Some("original"));
            assert!(duplicate.saved_id.is_none());
            assert!(duplicate.outputs.is_empty());
            assert!(duplicate.source_object.is_none());
            assert!(duplicate.executed_revision.is_none());
            assert_eq!(
                duplicate.editor.read(cx).value(),
                view.tabs[0].editor.read(cx).value()
            );
            while view.tabs.len() < MAX_TABS {
                view.new_query(window, cx);
            }
            let active_id = view.tabs[view.active].id;
            view.duplicate_query(window, cx);
            assert_eq!(view.tabs.len(), MAX_TABS);
            assert_eq!(view.tabs[view.active].id, active_id);
        });
    })
    .expect("Duplicate draft");
}

pub(super) fn open_studio(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (AnyWindowHandle, Entity<Studio>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: size(px(width), px(height)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    Studio::with_store(
                        WorkspaceStore::in_memory(),
                        WorkspaceData::default(),
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("Test window must open")
    })
}

#[gpui_kit::test]
fn scratch_runs_real_sql_and_retains_independent_drafts(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 1280., 820.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("scratch", cx);
    })
    .expect("Scratch action must dispatch");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        assert!(studio.read(cx).connection().is_some());
        studio.update(cx, |view, cx| {
            view.tabs[0]
                .editor
                .update(cx, |state, cx| state.focus(window, cx))
        });
        window.press("ctrl-enter", cx);
    })
    .expect("Run shortcut must dispatch");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        let tab = &studio.read(cx).tabs[0];
        assert!(!tab.is_error, "{}", tab.message);
        assert_eq!(
            tab.results.read(cx).delegate().rows,
            vec![vec![Some("1".into())]]
        );
        assert!(tab.editor.read(cx).value().contains("SELECT 1 AS result;"));
        window.render_frame(cx);
        window.click("new-query", cx);
        studio.update(cx, |view, cx| {
            view.tabs[1].editor.update(cx, |state, cx| {
                state.set_value("SELECT NULL AS value, 42 AS answer;", window, cx)
            })
        });
        window.press("ctrl-enter", cx);
    })
    .expect("Second query must dispatch");
    cx.run_until_parked();
    cx.update(|cx| {
        let view = studio.read(cx);
        assert_eq!(view.tabs.len(), 2);
        assert_eq!(
            view.tabs[0].results.read(cx).delegate().rows,
            vec![vec![Some("1".into())]]
        );
        assert_eq!(
            view.tabs[1].results.read(cx).delegate().rows,
            vec![vec![None, Some("42".into())]]
        );
        assert_eq!(view.workspace.history.len(), 2);
    });
}

#[gpui_kit::test]
fn opening_saved_queries_at_the_tab_limit_preserves_the_active_draft(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 1280., 820.);
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            while view.tabs.len() < MAX_TABS {
                view.add_query("Draft".into(), "SELECT 42;".into(), window, cx);
            }
            view.workspace.queries.push(crate::workspace::SavedQuery {
                id: "saved-query".into(),
                title: "Saved".into(),
                sql: "SELECT 7;".into(),
                connection_id: Some("another-server".into()),
            });
            view.output_tab = OutputTab::Saved;
        });
        window.render_frame(cx);
        window.click(("open-saved", 0usize), cx);
        let view = studio.read(cx);
        assert_eq!(view.tabs.len(), MAX_TABS);
        let tab = &view.tabs[view.active];
        assert_eq!(tab.editor.read(cx).value().as_str(), "SELECT 42;");
        assert!(tab.connection_id.is_none());
        assert!(tab.saved_id.is_none());
    })
    .expect("Saved query must preserve a draft when no tab can be created");
}

#[gpui_kit::test]
fn browse_pages_use_their_declared_size_and_preserve_query_context(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update(|cx| studio.update(cx, |view, cx| view.scratch(cx)));
    cx.run_until_parked();
    cx.update(|cx| studio.update(cx, |view, cx| view.execute_sql(
        "CREATE TABLE items (id INTEGER PRIMARY KEY); WITH RECURSIVE ids(id) AS (SELECT 1 UNION ALL SELECT id + 1 FROM ids WHERE id < 1500) INSERT INTO items SELECT id FROM ids;".into(), cx)));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let object = DbObject {
                schema: "main".into(),
                name: "items".into(),
                kind: crate::database::ObjectKind::Table,
            };
            let columns = vec![ColumnDetails {
                name: "id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                primary_key: true,
            }];
            let request = crate::table_browse::BrowseRequest {
                object: object.clone(),
                filter: None,
                sort: None,
                limit: 1200,
                page: 0,
            };
            let sql = crate::table_browse::browse_sql(
                crate::database::Engine::SQLite,
                &request,
                &columns,
            )
            .unwrap();
            view.add_query("Browse items".into(), sql, window, cx);
            let tab = &mut view.tabs[view.active];
            tab.source_object = Some(object);
            tab.browse = Some(TableBrowseState {
                engine: crate::database::Engine::SQLite,
                request,
                columns,
            });
            view.run_query(window, cx);
        });
    })
    .expect("Browse query dispatch");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        let view = studio.read(cx);
        let tab = &view.tabs[view.active];
        assert!(!tab.is_error, "{}", tab.message);
        assert_eq!(view.workspace.preferences.row_limit, 1000);
        assert_eq!(tab.outputs[0].rows.len(), 1200);
        window.render_frame(cx);
        for id in [
            "previous-table-page",
            "next-table-page",
            "export-results",
            "query-files",
            "run-statement",
        ] {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.size.width > px(0.) && bounds.size.height > px(0.),
                "{id} must be visible"
            );
            assert!(
                bounds.left() >= px(0.)
                    && bounds.right() <= px(900.)
                    && bounds.bottom() <= px(620.),
                "{id} must fit minimum browse layout: {bounds:?}"
            );
        }
        studio.update(cx, |view, cx| view.change_browse_page(true, window, cx));
    })
    .expect("Next page dispatch");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        let view = studio.read(cx);
        let tab = &view.tabs[view.active];
        assert!(!tab.is_error, "{}", tab.message);
        assert_eq!(tab.browse.as_ref().unwrap().request.page, 1);
        assert_eq!(tab.outputs[0].rows.len(), 300);
        assert_eq!(tab.outputs[0].rows[0][0].as_deref(), Some("1201"));
        assert_eq!(tab.executed_revision, Some(tab.revision));
        assert!(tab.source_object.is_some());
        studio.update(cx, |view, cx| view.change_browse_page(false, window, cx));
    })
    .expect("Previous page dispatch");
    cx.run_until_parked();
    cx.update(|cx| {
        let view = studio.read(cx);
        let tab = &view.tabs[view.active];
        assert_eq!(tab.outputs[0].rows.len(), 1200);
        assert_eq!(tab.outputs[0].rows[0][0].as_deref(), Some("1"));
    });
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let history_count = view.workspace.history.len();
            let tab = &mut view.tabs[view.active];
            // An incomplete memory-bounded page must never skip its unloaded rows.
            tab.outputs[0].rows.truncate(100);
            tab.outputs[0].truncated = true;
            view.change_browse_page(true, window, cx);
            assert_eq!(
                view.tabs[view.active].browse.as_ref().unwrap().request.page,
                0
            );
            assert!(view.tabs[view.active].cancel.is_none());
            assert_eq!(view.workspace.history.len(), history_count);
        });
    })
    .expect("Incomplete page guard");
}

#[gpui_kit::test]
fn current_statement_shortcut_executes_only_the_cursor_statement(cx: &mut TestAppContext) {
    let (handle, studio) = open_studio(cx, 1280., 820.);
    cx.update(|cx| studio.update(cx, |view, cx| view.scratch(cx)));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            view.tabs[view.active].editor.update(cx, |state, cx| {
                state.set_value("SELECT 1; SELECT 2;", window, cx);
                state.set_selected_range(15..15, cx);
                state.focus(window, cx);
            });
        });
        window.press("ctrl-shift-enter", cx);
    })
    .expect("Statement shortcut dispatch");
    cx.run_until_parked();
    cx.update(|cx| {
        let view = studio.read(cx);
        let tab = &view.tabs[view.active];
        assert!(!tab.is_error, "{}", tab.message);
        assert_eq!(tab.outputs.len(), 1);
        assert_eq!(tab.outputs[0].rows[0][0].as_deref(), Some("2"));
        assert_eq!(view.workspace.history.last().unwrap().sql, "SELECT 2;");
    });
}

#[gpui_kit::test]
fn essential_controls_fit_minimum_window_and_generated_queries_quote_names(
    cx: &mut TestAppContext,
) {
    let (handle, studio) = open_studio(cx, 900., 620.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let run = window.find("run-query").bounds();
        let open = window.find("open-table").bounds();
        assert!(run.size.width > px(0.) && run.right() <= px(900.));
        assert!(open.size.height > px(0.) && open.bottom() <= px(620.));
        for id in [
            "explain-query",
            "command-palette",
            "rename-query",
            "duplicate-query",
            "insert-row",
            "delete-row",
            "run-statement",
            "format-query",
            "rollback-transaction",
            "copy-results",
            "export-results",
            "query-files",
            "toggle-sidebar",
            "toggle-results",
        ] {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.size.width > px(0.) && bounds.size.height > px(0.),
                "{id} must be visible"
            );
            assert!(
                bounds.left() >= px(0.)
                    && bounds.top() >= px(0.)
                    && bounds.right() <= px(900.)
                    && bounds.bottom() <= px(620.),
                "{id} must fit minimum window: {bounds:?}"
            );
        }
        for index in 0..5usize {
            let bounds = window.within("output-tabs").find(index).bounds();
            assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
            assert!(
                bounds.left() >= px(0.)
                    && bounds.top() >= px(0.)
                    && bounds.right() <= px(900.)
                    && bounds.bottom() <= px(620.),
                "Output tab {index} must fit minimum window: {bounds:?}"
            );
        }
        studio.update(cx, |view, cx| view.scratch(cx));
    })
    .expect("Minimum layout must render");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            view.execute_sql(
                "CREATE TABLE \"odd name\" (id INTEGER PRIMARY KEY);".into(),
                cx,
            )
        });
        window.render_frame(cx);
    })
    .expect("DDL must dispatch");
    cx.run_until_parked();
    cx.update(|cx| studio.update(cx, |view, cx| view.refresh_catalog(cx)));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        studio.update(cx, |view, cx| {
            let object = view
                .connection()
                .expect("Scratch must remain connected")
                .catalog
                .iter()
                .find(|o| o.name == "odd name")
                .expect("DDL must populate real catalog")
                .clone();
            let sql = format!(
                "SELECT * FROM {} LIMIT 10;",
                view.connection().unwrap().session.quote_object(&object)
            );
            assert!(sql.contains("\"odd name\""));
            view.add_query("Quoted query".into(), sql, window, cx);
        });
    })
    .expect("Catalog interaction must complete");
}
