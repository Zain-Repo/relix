use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, Selectable, Sizable, ThemeMode,
    input::{Editor, Input},
    list::ListItem,
    menu::DropdownMenu,
    resizable::{h_resizable, resizable_panel, v_resizable},
    status_bar::StatusBar,
    tab::{Tab, TabBar},
    table::DataTable,
    toolbar::Toolbar,
    tree::tree,
    v_flex,
};
use gpui_kit::{div, px};

impl Studio {
    fn render_connection_label(
        profile: &ConnectionConfig,
        live: bool,
        description: String,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let name = div().text_sm().truncate().child(profile.name.clone());
        let metadata = h_flex()
            .gap_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .when(live && profile.read_only, |el| {
                el.child(Icon::new(IconName::Lock).size(px(12.)))
            })
            .child(div().min_w_0().truncate().child(description));
        let labels = v_flex()
            .flex_1()
            .min_w_0()
            .items_start()
            .gap_1()
            .child(name)
            .child(metadata);
        h_flex()
            .size_full()
            .gap_3()
            .child(Icon::new(IconName::Database).size(px(20.)))
            .child(labels)
            .into_any_element()
    }

    fn inspect_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let table = self.tabs[self.active].results.read(cx);
        let Some((row, column)) = table.selected_cell() else {
            self.notice =
                "Select a result cell, then choose View cell to inspect its full value.".into();
            cx.notify();
            return;
        };
        let Some((name, data_type, value)) = table.delegate().cell_details(row, column) else {
            return;
        };
        let raw_value = value.clone().unwrap_or_else(|| "NULL".into());
        let value_kind = match value.as_deref() {
            None => "SQL NULL".to_owned(),
            Some("") => "Empty string · 0 bytes".to_owned(),
            Some(value) => format!(
                "{} characters · {} bytes",
                value.chars().count(),
                value.len()
            ),
        };
        // Keep the exact database text available for selection and copying;
        // pretty-printing would change values such as JSON and whitespace.
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(true)
                .default_value(raw_value.clone())
        });
        window.open_dialog(cx, move |dialog, _, cx| {
            let copy_value = raw_value.clone();
            dialogs::frame(
                dialog,
                "Result inspector",
                name.to_owned(),
                "Select and copy the exact database value.",
                640.,
                cx,
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Row {} · {data_type} · {value_kind}", row + 1)),
                    )
                    .child(
                        Editor::new(&editor)
                            .h(px(280.))
                            .readonly(true)
                            .aria_label("Full result cell value"),
                    ),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("copy-cell-value")
                            .label("Copy value")
                            .disabled(value.is_none())
                            .tooltip("Copy exact text; SQL NULL has no text value")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                    copy_value.clone(),
                                ));
                            }),
                    )
                    .child(
                        Button::new("close-cell-details")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
        });
    }

    fn render_toolbar(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.any_running() || self.connecting;
        h_flex()
            .h(px(56.))
            .px_4()
            .gap_4()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .gap_2()
                    .child(Icon::new(IconName::Database).size(px(22.)))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Relix"),
                    ),
            )
            .child(
                Toolbar::new("app-toolbar")
                    .small()
                    .flex_1()
                    .child(
                        Button::new("connections")
                            .ghost()
                            .icon(IconName::Unplug)
                            .label("Connect")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.connection_dialog(None, window, cx)
                            })),
                    )
                    .child(
                        Button::new("scratch")
                            .ghost()
                            .label("Scratch")
                            .disabled(busy)
                            .tooltip("Create a new writable in-memory SQLite database")
                            .on_click(cx.listener(|this, _, _, cx| this.scratch(cx))),
                    )
                    .content(div().w(px(12.)))
                    .child(
                        Button::new("new-query")
                            .ghost()
                            .icon(IconName::Plus)
                            .label("New query")
                            .disabled(self.tabs.len() >= MAX_TABS)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.new_query(window, cx)),
                            ),
                    )
                    .content(
                        Button::new("query-files")
                            .ghost()
                            .small()
                            .label("Files")
                            .icon(IconName::ChevronDown)
                            .accessibility_label("Query file actions")
                            .dropdown_menu({
                                let focus = self.focus.clone();
                                move |menu, _, _| {
                                    menu.action_context(focus.clone())
                                        .menu("Open SQL file…", Box::new(OpenQuery))
                                        .menu("Save to workspace", Box::new(SaveQuery))
                                        .separator()
                                        .menu("Save SQL file as…", Box::new(SaveSqlFile))
                                }
                            }),
                    )
                    .content(div().flex_1())
                    .child(
                        Button::new("updates")
                            .ghost()
                            .label(self.update_state.toolbar_label())
                            .accessibility_label("Application updates")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_updates(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("command-palette")
                            .icon(IconName::Search)
                            .when(wide, |button| button.label("Search commands…").w(px(220.)))
                            .when(!wide, |button| button.ghost())
                            .accessibility_label("Open command palette")
                            .tooltip("Commands (Ctrl+K)")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_commands(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("toggle-sidebar")
                            .ghost()
                            .tooltip("Toggle explorer (Ctrl+B)")
                            .icon(IconName::PanelLeft)
                            .selected(self.show_sidebar)
                            .accessibility_label("Toggle database explorer")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sidebar = !this.show_sidebar;
                                this.persist();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("toggle-results")
                            .ghost()
                            .tooltip("Toggle results (Ctrl+J)")
                            .icon(IconName::PanelBottom)
                            .selected(self.show_results)
                            .accessibility_label("Toggle results panel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_results = !this.show_results;
                                this.persist();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .ghost()
                            .tooltip("Switch light or dark appearance")
                            .icon(if cx.theme().mode.is_dark() {
                                IconName::Sun
                            } else {
                                IconName::Moon
                            })
                            .accessibility_label("Switch light or dark appearance")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let mode = if cx.theme().mode.is_dark() {
                                    ThemeMode::Light
                                } else {
                                    ThemeMode::Dark
                                };
                                crate::studio_theme::apply(mode, Some(window), cx);
                                this.workspace.preferences.dark_mode = mode.is_dark();
                                this.persist();
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_connections(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let busy = self.any_running() || self.connecting;
        let active_id = self.active_connection.clone();
        let mut profiles = self.workspace.connections.clone();
        profiles.extend(
            self.connections
                .iter()
                .filter(|c| {
                    !self
                        .workspace
                        .connections
                        .iter()
                        .any(|profile| profile.id == c.config.id)
                })
                .map(|c| c.config.clone()),
        );
        let disconnect = Button::new("disconnect")
            .ghost()
            .xsmall()
            .icon(IconName::Unplug)
            .accessibility_label("Disconnect selected database")
            .tooltip("Disconnect")
            .disabled(busy || self.connection().is_none())
            .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx)));
        let connection_header = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Connections"),
            )
            .child(
                Button::new("add-connection")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .accessibility_label("Add database connection")
                    .tooltip("Connect a database")
                    .disabled(busy)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.connection_dialog(None, window, cx)),
                    ),
            )
            .child(disconnect);
        let empty_connections = profiles.is_empty();
        let connection_rows = profiles
            .into_iter()
            .map(|profile| {
                let id = profile.id.clone();
                let live = self
                    .connections
                    .iter()
                    .any(|connection| connection.config.id == id);
                let selected = active_id.as_ref() == Some(&id);
                let edit_profile = profile.clone();
                let remove_id = id.clone();
                let saved = self
                    .workspace
                    .connections
                    .iter()
                    .any(|profile| profile.id == id);
                let description = format!(
                    "{} · {}",
                    profile.engine.name(),
                    if live {
                        if profile.read_only {
                            "Read-only"
                        } else {
                            "Writable"
                        }
                    } else {
                        "Saved profile"
                    }
                );
                let select = Button::new(gpui_kit::SharedString::from(format!("profile-{id}")))
                    .ghost()
                    .small()
                    .h(px(52.))
                    .flex_1()
                    .min_w_0()
                    .justify_start()
                    .accessibility_label(format!("{} · {description}", profile.name))
                    .child(Self::render_connection_label(
                        &profile,
                        live,
                        description,
                        cx,
                    ))
                    .tooltip(format!(
                        "{} · {} · {}",
                        profile.name,
                        profile.engine.name(),
                        if live { "Connected" } else { "Saved profile" }
                    ))
                    .selected(selected)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if live {
                            this.select_connection(id.clone(), cx);
                        } else {
                            this.connection_dialog(Some(profile.clone()), window, cx);
                        }
                    }));
                let edit = Button::new(gpui_kit::SharedString::from(format!(
                    "edit-profile-{}",
                    edit_profile.id
                )))
                .ghost()
                .xsmall()
                .icon(IconName::Pencil)
                .accessibility_label("Edit connection profile")
                .tooltip("Edit connection profile")
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.connection_dialog(Some(edit_profile.clone()), window, cx)
                }));
                let remove = Button::new(gpui_kit::SharedString::from(format!(
                    "remove-profile-{remove_id}"
                )))
                .ghost()
                .xsmall()
                .icon(IconName::X)
                .accessibility_label("Remove saved connection profile")
                .tooltip("Remove saved profile; database data is kept")
                .disabled(busy || !saved)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.remove_profile(remove_id.clone(), window, cx)
                }));
                h_flex().gap_1().child(select).child(edit).child(remove)
            })
            .collect::<Vec<_>>();
        let connection_list =
            div()
                .id("connection-list")
                .max_h(px(224.))
                .overflow_y_scroll()
                .px_2()
                .child(
                    v_flex()
                        .gap_1()
                        .when(empty_connections, |element| {
                            element.child(div().p_2().text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Connect an existing database or create a scratch database."))
                        })
                        .children(connection_rows),
                );
        v_flex()
            .flex_shrink_0()
            .child(connection_header)
            .child(connection_list)
            .into_any_element()
    }

    fn render_catalog(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let busy = self.any_running() || self.connecting;
        let object_search = self.filter.read(cx).value().trim().to_lowercase();
        let no_matches = self.connection().is_some_and(|connection| {
            !connection.catalog.is_empty()
                && !connection.catalog.iter().any(|object| {
                    format!("{}.{}", object.schema, object.name)
                        .to_lowercase()
                        .contains(&object_search)
                })
        });
        let views = self
            .connection()
            .map(|connection| {
                connection
                    .catalog
                    .iter()
                    .enumerate()
                    .filter(|(_, object)| object.kind == crate::database::ObjectKind::View)
                    .map(|(index, _)| format!("object-{index}"))
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        let object_labels = self
            .connection()
            .map(|connection| {
                connection
                    .catalog
                    .iter()
                    .enumerate()
                    .map(|(index, object)| {
                        (
                            format!("object-{index}"),
                            format!("{}.{}", object.schema, object.name),
                        )
                    })
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default();
        let refresh = Button::new("refresh-catalog")
            .ghost()
            .xsmall()
            .icon(IconName::RefreshCw)
            .accessibility_label("Refresh database catalog")
            .disabled(busy || self.connection().is_none())
            .on_click(cx.listener(|this, _, _, cx| this.refresh_catalog(cx)));
        let catalog_header = h_flex()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Database Explorer"),
            )
            .child(refresh);
        let object_filter = div().px_3().pb_2().child(
            Input::new(&self.filter)
                .small()
                .prefix(IconName::Search)
                .cleanable(true)
                .aria_label("Filter database objects"),
        );
        let catalog = tree(&self.objects, move |index, entry, selected, _, cx| {
            let id = entry.item().id.as_str();
            let leaf = id.starts_with("object-");
            let icon = if views.contains(id) || id.starts_with("views-") {
                IconName::Eye
            } else if leaf || id.starts_with("tables-") {
                IconName::Table
            } else if id.starts_with("connection-") || id.starts_with("database-") {
                IconName::Database
            } else {
                IconName::Folder
            };
            let tooltip = object_labels
                .get(id)
                .cloned()
                .unwrap_or_else(|| entry.item().label.to_string());
            ListItem::new(index)
                .h(px(28.))
                .text_sm()
                .selected(selected)
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
                .accessibility_label(entry.item().label.clone())
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_1p5()
                        .child(
                            Icon::new(icon)
                                .size(px(16.))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_weight(
                                    if id.starts_with("connection-") || id.starts_with("schema-") {
                                        gpui_kit::FontWeight::MEDIUM
                                    } else {
                                        gpui_kit::FontWeight::NORMAL
                                    },
                                )
                                .child(entry.item().label.clone()),
                        ),
                )
        })
        .text_xs();
        let navigator = div()
            .id("database-navigator")
            .flex_1()
            .min_h_0()
            .px_1()
            .text_xs()
            .when(self.connection().is_none(), |el| {
                el.child(
                    div()
                        .px_3()
                        .py_4()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Connect a database to explore its schemas."),
                )
            })
            .when(
                self.connection()
                    .is_some_and(|connection| connection.catalog.is_empty()),
                |el| {
                    el.child(
                        div()
                            .px_3()
                            .py_4()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No tables or views in this database."),
                    )
                },
            )
            .when(no_matches, |el| {
                el.child(
                    div()
                        .px_3()
                        .py_4()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No objects match your search."),
                )
            })
            .child(catalog)
            .test_support();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(catalog_header)
            .child(object_filter)
            .child(navigator)
            .into_any_element()
    }

    fn render_sidebar_actions(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let busy = self.any_running() || self.connecting;
        let can_open = self.selected_object(cx).is_some();
        let query_table = Button::new("open-table")
            .ghost()
            .small()
            .flex_1()
            .label("Query table")
            .disabled(!can_open)
            .on_click(cx.listener(|this, _, window, cx| this.open_table(window, cx)));
        let inspect = Button::new("inspect-schema")
            .ghost()
            .small()
            .label("Schema")
            .disabled(!can_open || busy)
            .on_click(cx.listener(|this, _, _, cx| this.inspect_object(cx)));
        let import = Button::new("import-csv")
            .ghost()
            .small()
            .icon(IconName::FolderOpen)
            .accessibility_label("Import CSV")
            .tooltip("Import CSV into a new table using reviewable SQL")
            .disabled(
                busy || self
                    .connection()
                    .is_none_or(|connection| connection.config.read_only),
            )
            .on_click(cx.listener(|this, _, window, cx| this.open_sql_file(true, window, cx)));
        let saved = Button::new("saved-queries")
            .ghost()
            .small()
            .label(format!("Saved queries ({})", self.workspace.queries.len()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.output_tab = OutputTab::Saved;
                this.show_results = true;
                cx.notify();
            }));
        let footer = v_flex()
            .p_3()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(h_flex().gap_2().child(query_table).child(inspect))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("browse-table")
                            .small()
                            .icon(IconName::Table)
                            .label("Browse table…")
                            .disabled(!can_open || busy)
                            .tooltip("Prepare a table query with filters, sorting and pagination")
                            .flex_1()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.browse_table(window, cx)),
                            ),
                    )
                    .child(import),
            )
            .child(saved);
        footer.into_any_element()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let connections = self.render_connections(cx);
        let catalog = self.render_catalog(cx);
        let actions = self.render_sidebar_actions(cx);
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(connections)
            .child(catalog)
            .child(actions)
    }

    fn render_query_tabs(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let tabs = TabBar::new("query-tabs")
            .underline()
            .small()
            .selected_index(self.active)
            .max_width(px(180.))
            .menu(true)
            .on_click(cx.listener(|this, index, window, cx| {
                this.active = *index;
                this.tabs[*index]
                    .editor
                    .update(cx, |state, cx| state.focus(window, cx));
                this.select_result(this.tabs[*index].result_index, cx);
                cx.notify();
            }))
            .children(self.tabs.iter().map(|tab| {
                let id = tab.id;
                Tab::new()
                    .label(format!(
                        "{}{}",
                        tab.title,
                        if tab.dirty { " *" } else { "" }
                    ))
                    .prefix(IconName::FileCode)
                    .suffix(
                        Button::new(("close-query", id))
                            .ghost()
                            .xsmall()
                            .icon(IconName::X)
                            .accessibility_label("Close query")
                            .disabled(self.tabs.len() == 1 || tab.cancel.is_some())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_query(id, window, cx)
                            })),
                    )
            }));
        tabs.into_any_element()
    }

    fn render_query_commands(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let active = &self.tabs[self.active];
        let busy = self.any_running() || self.connecting;
        let connected = self.connection().is_some();
        let format = Button::new("format-query")
            .ghost()
            .small()
            .icon(IconName::ListFilter)
            .label("Format")
            .tooltip("Format SQL (Ctrl+Shift+F)")
            .on_click(cx.listener(|this, _, window, cx| this.format_query(window, cx)));
        let commands = h_flex()
            .h(px(52.))
            .px_3()
            .gap_2()
            .flex_shrink_0()
            .child(
                Button::new("run-query")
                    .primary()
                    .small()
                    .icon(IconName::Play)
                    .label(if active.cancel.is_some() {
                        "Running…"
                    } else {
                        "Run"
                    })
                    .disabled(!connected || busy)
                    .tooltip("Run selected SQL, or the full script (Ctrl+Enter)")
                    .on_click(cx.listener(|this, _, window, cx| this.run_query(window, cx))),
            )
            .child(
                Button::new("run-statement")
                    .ghost()
                    .small()
                    .icon(IconName::FileCode)
                    .accessibility_label("Run current SQL statement")
                    .tooltip("Run the statement at the cursor (Ctrl+Shift+Enter)")
                    .disabled(!connected || busy)
                    .on_click(cx.listener(|this, _, window, cx| this.run_statement(window, cx))),
            )
            .child(
                Button::new("stop-query")
                    .ghost()
                    .small()
                    .icon(IconName::Square)
                    .label("Stop")
                    .disabled(active.cancel.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.stop_query(cx))),
            )
            .child(format)
            .child(
                Button::new("explain-query")
                    .ghost()
                    .small()
                    .icon(IconName::Search)
                    .label("Explain")
                    .disabled(!connected || busy)
                    .on_click(cx.listener(|this, _, _, cx| this.explain_query(cx))),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("query-connection-context")
                    .min_w_0()
                    .max_w(px(240.))
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .when_some(self.connection(), |el, connection| {
                        el.child(
                            Icon::new(if connection.config.read_only {
                                IconName::Lock
                            } else {
                                IconName::Database
                            })
                            .small(),
                        )
                    })
                    .child(
                        div().min_w_0().truncate().child(
                            self.connection()
                                .map(|connection| {
                                    format!(
                                        "{} · {} · {}",
                                        connection.config.name,
                                        connection.config.engine.name(),
                                        if connection.config.read_only {
                                            "Read-only"
                                        } else {
                                            "Writable"
                                        },
                                    )
                                })
                                .unwrap_or_else(|| "Disconnected".into()),
                        ),
                    ),
            );
        commands.into_any_element()
    }

    fn render_query_options(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let active = &self.tabs[self.active];
        let busy = self.any_running() || self.connecting;
        let connected = self.connection().is_some();
        let writable = self.connection().is_some_and(|c| !c.config.read_only);
        let transaction_buttons = [
            ("begin-transaction", "Begin", "BEGIN"),
            ("commit-transaction", "Commit", "COMMIT"),
            ("rollback-transaction", "Rollback", "ROLLBACK"),
        ]
        .into_iter()
        .map(|(id, label, sql)| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .disabled(!writable || busy)
                .on_click(cx.listener(move |this, _, _, cx| this.execute_sql(sql.into(), cx)))
        })
        .collect::<Vec<_>>();
        let needs_retarget = active
            .connection_id
            .as_ref()
            .is_some_and(|id| Some(id) != self.active_connection.as_ref());
        let retarget = Button::new("retarget-query")
            .ghost()
            .xsmall()
            .label("Retarget query")
            .disabled(busy || !connected)
            .tooltip("Explicitly assign this draft to the selected connection")
            .on_click(cx.listener(|this, _, _, cx| {
                this.tabs[this.active].connection_id = this.active_connection.clone();
                this.tabs[this.active].source_object = None;
                this.tabs[this.active].browse = None;
                this.refresh_completions(cx);
                this.notice =
                    "Query target changed. Review SQL before running on the selected connection."
                        .into();
                cx.notify();
            }));
        let draft_target = active.connection_id.as_ref().and_then(|id| {
            self.connections
                .iter()
                .map(|connection| &connection.config)
                .chain(self.workspace.connections.iter())
                .find(|config| &config.id == id)
        });
        let transaction = h_flex()
            .min_h(px(32.))
            .flex_wrap()
            .px_3()
            .gap_2()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("rename-query")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Pencil)
                    .accessibility_label("Rename current query")
                    .tooltip("Rename current query")
                    .on_click(cx.listener(|this, _, window, cx| this.rename_query(window, cx))),
            )
            .child(
                Button::new("duplicate-query")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Copy)
                    .accessibility_label("Duplicate current query")
                    .tooltip("Duplicate into a new draft")
                    .disabled(self.tabs.len() >= MAX_TABS)
                    .on_click(cx.listener(|this, _, window, cx| this.duplicate_query(window, cx))),
            )
            .child(div().w(px(12.)))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Transaction"),
            )
            .children(transaction_buttons)
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if active.browse.is_some() {
                        "Query limit"
                    } else {
                        "Row limit"
                    }),
            )
            .children([500usize, 1000, 5000].into_iter().map(|limit| {
                Button::new(("row-limit", limit))
                    .ghost()
                    .xsmall()
                    .label(limit.to_string())
                    .selected(self.workspace.preferences.row_limit == limit)
                    .disabled(active.browse.is_some())
                    .tooltip(
                        "SQL draft row limit; table pages use the page size chosen in Browse table",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.workspace.preferences.row_limit = limit;
                        this.persist();
                        cx.notify();
                    }))
            }))
            .when_some(active.browse.as_ref(), |el, browse| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Page size {}", browse.request.limit)),
                )
            })
            .when(needs_retarget, |element| {
                element
                    .child(
                        div()
                            .id("draft-target-mismatch")
                            .min_w_0()
                            .max_w(px(220.))
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "Draft target: {}",
                                draft_target
                                    .map(|config| config.name.as_str())
                                    .unwrap_or("Disconnected connection"),
                            ))
                            .test_support(),
                    )
                    .child(retarget)
            });
        transaction.into_any_element()
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = &self.tabs[self.active];
        let tabs = self.render_query_tabs(cx);
        let commands = self.render_query_commands(cx);
        let transaction = self.render_query_options(cx);
        v_flex()
            .size_full()
            .min_w_0()
            .child(tabs)
            .child(commands)
            .child(transaction)
            .child(
                div().flex_1().min_h_0().min_w_0().overflow_hidden().child(
                    Editor::new(&active.editor)
                        .h(gpui_kit::relative(1.))
                        .bordered(false)
                        .aria_label("SQL query editor"),
                ),
            )
    }

    fn render_export_button(&self) -> impl IntoElement {
        let active = &self.tabs[self.active];
        Button::new("export-results")
            .ghost()
            .xsmall()
            .label("Export")
            .icon(IconName::ChevronDown)
            .disabled(active.outputs.is_empty())
            .accessibility_label("Export loaded results")
            .dropdown_menu({
                let focus = self.focus.clone();
                let insert_unavailable = active.source_object.is_none()
                    || active.executed_revision != Some(active.revision)
                    || active.connection_id.as_ref().is_none_or(|id| {
                        !self
                            .connections
                            .iter()
                            .any(|connection| &connection.config.id == id)
                    });
                move |menu, _, _| {
                    menu.action_context(focus.clone())
                        .label("Loaded results")
                        .menu("CSV…", Box::new(ExportCsv))
                        .menu("JSON…", Box::new(ExportJson))
                        .menu("Markdown…", Box::new(ExportMarkdown))
                        .separator()
                        .menu_with_disabled(
                            "SQL INSERT statements…",
                            Box::new(ExportInsert),
                            insert_unavailable,
                        )
                }
            })
    }

    fn render_result_filter(&self, wide: bool) -> impl IntoElement {
        div()
            .id("result-filter-control")
            .w(px(if wide { 190. } else { 210. }))
            .child(
                Input::new(&self.result_filter)
                    .small()
                    .prefix(IconName::Search)
                    .cleanable(true)
                    .aria_label("Filter loaded result rows"),
            )
            .test_support()
    }

    fn render_output_content(&self, wide: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let active = &self.tabs[self.active];
        let row_count = active.results.read(cx).delegate().rows.len();
        let loaded_count = active.results.read(cx).delegate().loaded_count();
        let history_search = self.history_filter.read(cx).value().trim().to_lowercase();
        let saved_search = self.saved_filter.read(cx).value().trim().to_lowercase();
        let history_matches = |entry: &crate::workspace::HistoryRecord| {
            entry.sql.to_lowercase().contains(&history_search)
                || entry
                    .connection_name
                    .to_lowercase()
                    .contains(&history_search)
        };
        let saved_matches = |query: &crate::workspace::SavedQuery| {
            query.title.to_lowercase().contains(&saved_search)
                || query.sql.to_lowercase().contains(&saved_search)
        };
        match self.output_tab {
            OutputTab::Results => v_flex()
                .size_full()
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .when(!wide, |el| el.child(self.render_result_filter(false)))
                        .child(
                            div().text_xs().text_color(cx.theme().muted_foreground)
                                .child(format!("{row_count} / {loaded_count} loaded rows")),
                        )
                        .when(
                            active
                                .executed_revision
                                .is_some_and(|revision| revision != active.revision),
                            |el| el.child(div().text_xs().child("SQL edited · results are stale")),
                        )
                        .when(active.outputs.len() > 1, |el| {
                            el.child(
                                Button::new("previous-result")
                                    .ghost()
                                    .xsmall()
                                    .label("Previous")
                                    .disabled(active.result_index == 0)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_result(
                                            this.tabs[this.active].result_index.saturating_sub(1),
                                            cx,
                                        )
                                    })),
                            )
                            .child(div().text_xs().child(format!(
                                "Result {} / {}",
                                active.result_index + 1,
                                active.outputs.len()
                            )))
                            .child(
                                Button::new("next-result")
                                    .ghost()
                                    .xsmall()
                                    .label("Next")
                                    .disabled(active.result_index + 1 >= active.outputs.len())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_result(
                                            this.tabs[this.active].result_index + 1,
                                            cx,
                                        )
                                    })),
                            )
                        }),
                )
                .child(
                    div().flex_1().min_h_0().child(
                        DataTable::new(&active.results)
                            .small()
                            .stripe(true)
                            .bordered(false),
                    ),
                )
                .into_any_element(),
            OutputTab::Messages => div()
                .id("query-messages")
                .size_full()
                .p_4()
                .overflow_y_scroll()
                .role(if active.is_error {
                    gpui_kit::Role::Alert
                } else {
                    gpui_kit::Role::Status
                })
                .aria_label(active.message.clone())
                .child(active.message.clone())
                .into_any_element(),
            OutputTab::History => div()
                .id("query-history-list")
                .size_full()
                .p_3()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Search query history"))
                        .child(Input::new(&self.history_filter).small().aria_label("Search history SQL or connection"))
                        .when(self.workspace.history.is_empty(), |el| {
                            el.child("Executed queries will appear here.")
                        })
                        .when(!self.workspace.history.is_empty() && !self.workspace.history.iter().any(history_matches), |el| {
                            el.child(div().text_xs().text_color(cx.theme().muted_foreground).child("No history entries match your search."))
                        })
                        .children(self.workspace.history.iter().enumerate().rev().filter(|(_, entry)| history_matches(entry)).map(
                            |(index, entry)| {
                                let source = entry.sql.clone();
                                let connection_id = entry.connection_id.clone();
                                h_flex()
                                    .gap_3()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .pb_2()
                                    .child(
                                        v_flex()
                                            .w(px(170.))
                                            .text_xs()
                                            .child(entry.connection_name.clone())
                                            .child(format!(
                                                "{} · {} ms · {} rows",
                                                if entry.succeeded { "Success" } else { "Failed" },
                                                entry.elapsed_ms,
                                                entry.row_count
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .child(
                                                entry
                                                    .sql
                                                    .lines()
                                                    .find(|l| !l.trim().is_empty())
                                                    .unwrap_or("Empty query")
                                                    .to_owned(),
                                            ),
                                    )
                                    .child(
                                        Button::new(("reopen-query", index))
                                            .ghost()
                                            .small()
                                            .label("Open")
                                            .disabled(self.tabs.len() >= MAX_TABS)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.add_query(
                                                    "History query".into(),
                                                    source.clone(),
                                                    window,
                                                    cx,
                                                );
                                                this.tabs[this.active].connection_id =
                                                    Some(connection_id.clone());
                                                this.refresh_completions(cx);
                                            })),
                                    )
                            },
                        )),
                )
                .into_any_element(),
            OutputTab::Schema => div()
                .id("schema-inspector")
                .size_full()
                .p_3()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .gap_2()
                        .child(self.schema_title.clone())
                        .when(self.schema.is_empty(), |el| {
                            el.child("Select a table or view and choose Schema.")
                        })
                        .children(self.schema.iter().map(|column| {
                            h_flex()
                                .gap_4()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .pb_2()
                                .child(div().w(px(200.)).child(column.name.clone()))
                                .child(div().flex_1().child(column.data_type.clone()))
                                .child(if column.primary_key {
                                    "Primary key"
                                } else if column.nullable {
                                    "Nullable"
                                } else {
                                    "Not null"
                                })
                        }))
                        .when(!self.schema.is_empty(), |el| {
                            el.child(div().pt_3().font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child(format!("Indexes ({})", self.schema_indexes.len())))
                                .when(self.schema_indexes.is_empty(), |el| {
                                    el.child(div().text_xs().text_color(cx.theme().muted_foreground)
                                        .child("No indexes reported for this object."))
                                })
                                .children(self.schema_indexes.iter().map(|index| {
                                    v_flex().gap_1().py_2()
                                        .child(h_flex().gap_3()
                                            .child(div().font_weight(gpui_kit::FontWeight::MEDIUM).child(index.name.clone()))
                                            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                                                .child(if index.primary_key { "Primary key" } else if index.unique { "Unique" } else { "Index" })))
                                        .child(div().text_xs().child(index.columns.join(", ")))
                                        .when_some(index.definition.clone(), |el, definition| {
                                            el.child(div().text_xs().font_family(cx.theme().mono_font_family.clone()).child(definition))
                                        })
                                }))
                                .child(div().pt_3().font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(format!("Foreign keys ({})", self.schema_foreign_keys.len())))
                                .when(self.schema_foreign_keys.is_empty(), |el| {
                                    el.child(div().text_xs().text_color(cx.theme().muted_foreground)
                                        .child("No foreign keys reported for this object."))
                                })
                                .children(self.schema_foreign_keys.iter().map(|key| {
                                    v_flex().gap_1().py_2()
                                        .child(div().font_weight(gpui_kit::FontWeight::MEDIUM).child(key.name.clone()))
                                        .child(div().text_xs().child(format!("{} → {}.{} ({})", key.columns.join(", "), key.referenced_schema, key.referenced_table, key.referenced_columns.join(", "))))
                                        .child(div().text_xs().text_color(cx.theme().muted_foreground)
                                            .child(format!("On update: {} · On delete: {}", key.on_update, key.on_delete)))
                                }))
                        }),
                )
                .into_any_element(),
            OutputTab::Saved => div()
                .id("saved-query-list")
                .size_full()
                .p_3()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Search saved queries"))
                        .child(Input::new(&self.saved_filter).small().aria_label("Search saved query title or SQL"))
                        .when(self.workspace.queries.is_empty(), |el| {
                            el.child("Save a query with Ctrl+S to keep it between sessions.")
                        })
                        .when(!self.workspace.queries.is_empty() && !self.workspace.queries.iter().any(saved_matches), |el| {
                            el.child(div().text_xs().text_color(cx.theme().muted_foreground).child("No saved queries match your search."))
                        })
                        .children(self.workspace.queries.iter().enumerate().filter(|(_, query)| saved_matches(query)).map(
                            |(index, query)| {
                                let query = query.clone();
                                let id = query.id.clone();
                                h_flex()
                                    .gap_3()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .pb_2()
                                    .child(div().flex_1().truncate().child(query.title.clone()))
                                    .child(
                                        Button::new(("open-saved", index))
                                            .ghost()
                                            .small()
                                            .label("Open")
                                            .disabled(self.tabs.len() >= MAX_TABS)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if this.tabs.len() >= MAX_TABS {
                                                    this.notice = "Close a query before opening a saved query.".into();
                                                    cx.notify();
                                                    return;
                                                }
                                                this.add_query(
                                                    query.title.clone(),
                                                    query.sql.clone(),
                                                    window,
                                                    cx,
                                                );
                                                this.tabs[this.active].connection_id =
                                                    query.connection_id.clone();
                                                this.tabs[this.active].saved_id =
                                                    Some(query.id.clone());
                                                this.refresh_completions(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new(("delete-saved", index))
                                            .ghost()
                                            .small()
                                            .label("Remove")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.workspace.queries.retain(|q| q.id != id);
                                                this.persist();
                                                cx.notify();
                                            })),
                                    )
                            },
                        )),
                )
                .into_any_element(),
        }
    }

    fn render_output_header(&self, wide: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let loaded_count = self.tabs[self.active]
            .results
            .read(cx)
            .delegate()
            .loaded_count();
        h_flex()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div().id("output-tab-navigation").child(
                    TabBar::new("output-tabs")
                        .underline()
                        .small()
                        .selected_index(self.output_tab.index())
                        .on_click(cx.listener(|this, index, _, cx| {
                            this.output_tab = match index {
                                1 => OutputTab::Messages,
                                2 => OutputTab::History,
                                3 => OutputTab::Schema,
                                4 => OutputTab::Saved,
                                _ => OutputTab::Results,
                            };
                            cx.notify();
                        }))
                        .children(
                            [
                                format!("Results ({loaded_count})"),
                                "Messages".into(),
                                "History".into(),
                                "Schema".into(),
                                "Saved".into(),
                            ]
                            .into_iter()
                            .map(|label| Tab::new().label(label)),
                        ),
                ),
            )
            .child(div().flex_1())
            .when(wide && self.output_tab == OutputTab::Results, |el| {
                el.child(self.render_export_button())
                    .child(self.render_result_filter(true))
            })
            .child(
                Button::new("hide-output")
                    .ghost()
                    .small()
                    .icon(IconName::X)
                    .accessibility_label("Hide results panel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_results = false;
                        this.persist();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_output_footer(&self, wide: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let active = &self.tabs[self.active];
        let row_count = active.results.read(cx).delegate().rows.len();
        let loaded_count = active.results.read(cx).delegate().loaded_count();
        // Record commands only target the original, unchanged base-table query.
        // The handlers recheck this context before preparing reviewable SQL.
        let can_modify_records = !self.any_running()
            && !self.connecting
            && self.tabs.len() < MAX_TABS
            && active.connection_id == self.active_connection
            && active.executed_revision == Some(active.revision)
            && active
                .source_object
                .as_ref()
                .is_some_and(|object| object.kind == crate::database::ObjectKind::Table)
            && self
                .connection()
                .is_some_and(|connection| !connection.config.read_only);
        let has_selected_cell = active.results.read(cx).selected_cell().is_some();
        h_flex()
            .min_h(px(32.))
            .py_1()
            .flex_wrap()
            .px_3()
            .gap_2()
            .flex_shrink_0()
            .text_xs()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(format!(
                "{row_count} rows · {} ms{}",
                active.elapsed_ms,
                if active
                    .outputs
                    .get(active.result_index)
                    .is_some_and(|r| r.truncated)
                {
                    " · limited"
                } else {
                    ""
                }
            ))
            .when_some(active.browse.as_ref(), |el, browse| {
                let unavailable = self.any_running()
                    || self.connecting
                    || active.connection_id != self.active_connection
                    || self.connection().is_none()
                    || active.executed_revision != Some(active.revision);
                let incomplete = loaded_count < browse.request.limit
                    && active
                        .outputs
                        .get(active.result_index)
                        .is_some_and(|result| result.truncated);
                el.child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("previous-table-page")
                                .ghost()
                                .xsmall()
                                .label("Previous page")
                                .disabled(unavailable || browse.request.page == 0)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.change_browse_page(false, window, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(format!("Page {}", browse.request.page.saturating_add(1))),
                        )
                        .child(
                            Button::new("next-table-page")
                                .ghost()
                                .xsmall()
                                .label("Next page")
                                .disabled(unavailable || loaded_count < browse.request.limit)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.change_browse_page(true, window, cx)
                                })),
                        )
                        .when(incomplete, |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Page incomplete · reduce page size"),
                            )
                        }),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("inspect-cell")
                    .ghost()
                    .xsmall()
                    .label("View cell…")
                    .tooltip("Inspect and copy the full selected value")
                    .disabled(row_count == 0)
                    .on_click(cx.listener(|this, _, window, cx| this.inspect_cell(window, cx))),
            )
            .child(
                Button::new("edit-cell")
                    .ghost()
                    .xsmall()
                    .label("Edit cell…")
                    .tooltip("Prepare an UPDATE for review before running it")
                    .disabled(!can_modify_records || !has_selected_cell)
                    .on_click(cx.listener(|this, _, window, cx| this.edit_cell(window, cx))),
            )
            .child(
                Button::new("insert-row")
                    .ghost()
                    .xsmall()
                    .label("Insert row…")
                    .tooltip("Prepare an INSERT for review before running it")
                    .disabled(!can_modify_records)
                    .on_click(cx.listener(|this, _, window, cx| this.insert_row(window, cx))),
            )
            .child(
                Button::new("delete-row")
                    .ghost()
                    .xsmall()
                    .label("Delete row…")
                    .tooltip("Prepare a guarded DELETE for the selected record")
                    .disabled(!can_modify_records || !has_selected_cell)
                    .on_click(cx.listener(|this, _, window, cx| this.delete_row(window, cx))),
            )
            .child(div().w(px(8.)))
            .child(
                Button::new("copy-results")
                    .ghost()
                    .xsmall()
                    .label("Copy TSV")
                    .disabled(active.outputs.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let result = this.tabs[this.active].results.read(cx).delegate();
                        let mut lines = vec![result.headers().join("\t")];
                        lines.extend(result.rows.iter().map(|row| {
                            row.iter()
                                .map(|cell| {
                                    cell.as_deref()
                                        .unwrap_or("NULL")
                                        .replace('\t', "\\t")
                                        .replace('\n', "\\n")
                                })
                                .collect::<Vec<_>>()
                                .join("\t")
                        }));
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                            lines.join("\n"),
                        ));
                    })),
            )
            .when(!wide || self.output_tab != OutputTab::Results, |el| {
                el.child(self.render_export_button())
            })
            .into_any_element()
    }

    fn render_output(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_output_content(wide, cx);
        let header = self.render_output_header(wide, cx);
        let footer = self.render_output_footer(wide, cx);
        v_flex()
            .size_full()
            .min_w_0()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(content),
            )
            .child(footer)
    }
}

impl gpui_kit::Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.installing_update {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child("Preparing to restart and install the update…")
                .into_any_element();
        }
        // Reserve room for the widest explorer so controls remain reachable
        // even after dragging the pane divider at this window size.
        let wide = window.viewport_size().width >= px(1280.);
        let output_height = (window.viewport_size().height.as_f32() * 0.44).clamp(220., 440.);
        let connected = self
            .connection()
            .map(|c| c.config.name.clone())
            .unwrap_or("Disconnected".into());
        // Build pane sections independently to bound debug-build stack usage
        // on Windows while composing the native workspace.
        let toolbar = self.render_toolbar(wide, cx).into_any_element();
        let sidebar = self.render_sidebar(cx).into_any_element();
        let editor = self.render_editor(cx).into_any_element();
        let output = self.render_output(wide, cx).into_any_element();
        v_flex()
            .id("relix-studio")
            .track_focus(&self.focus)
            .key_context("Studio")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone())
            .text_sm()
            .on_action(cx.listener(|this, _: &NewQuery, window, cx| this.new_query(window, cx)))
            .on_action(cx.listener(|this, _: &RunQuery, window, cx| this.run_query(window, cx)))
            .on_action(
                cx.listener(|this, _: &RunStatement, window, cx| this.run_statement(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SaveQuery, _, cx| this.save_query(cx)))
            .on_action(
                cx.listener(|this, _: &ExportCsv, window, cx| this.write_file("csv", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ExportJson, window, cx| this.write_file("json", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ExportMarkdown, window, cx| {
                    this.write_file("md", window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ExportInsert, window, cx| {
                this.write_file("insert", window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SaveSqlFile, window, cx| this.write_file("sql", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenCommands, window, cx| this.open_commands(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatQuery, window, cx| this.format_query(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenQuery, window, cx| {
                    this.open_sql_file(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.show_sidebar = !this.show_sidebar;
                this.persist();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleResults, _, cx| {
                this.show_results = !this.show_results;
                this.persist();
                cx.notify();
            }))
            .child(toolbar)
            .when(self.storage_error.is_some(), |el| {
                el.child(
                    div()
                        .id("workspace-load-error")
                        .role(gpui_kit::Role::Alert)
                        .px_3()
                        .py_2()
                        .text_xs()
                        .child(self.storage_error.clone().unwrap_or_default()),
                )
            })
            .when(!self.notice.is_empty(), |el| {
                el.child(
                    div()
                        .id("workspace-notice")
                        .role(gpui_kit::Role::Status)
                        .aria_label(self.notice.clone())
                        .px_3()
                        .py_2()
                        .text_xs()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(self.notice.clone()),
                )
            })
            .child(
                div().flex_1().min_h_0().min_w_0().child(
                    h_resizable("studio-horizontal")
                        .child(
                            resizable_panel()
                                .visible(self.show_sidebar)
                                .size(px(280.))
                                .size_range(px(220.)..px(380.))
                                .child(sidebar),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(550.)..gpui_kit::Pixels::MAX)
                                .child(
                                    v_resizable("studio-vertical")
                                        .child(
                                            resizable_panel()
                                                .size_range(px(180.)..gpui_kit::Pixels::MAX)
                                                .child(editor),
                                        )
                                        .child(
                                            resizable_panel()
                                                .visible(self.show_results)
                                                .size(px(output_height))
                                                .size_range(px(140.)..gpui_kit::Pixels::MAX)
                                                .child(output),
                                        ),
                                ),
                        ),
                ),
            )
            .child(
                StatusBar::new()
                    .left(connected)
                    .left(if self.connecting {
                        "Connecting / loading…"
                    } else if self.any_running() {
                        "Query running"
                    } else {
                        "Ready"
                    })
                    .right("SQL · UTF-8")
                    .right("Unsaved drafts are session-only"),
            )
            .into_any_element()
    }
}
