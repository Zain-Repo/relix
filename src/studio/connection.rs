use super::*;
use crate::database::Engine;
use crate::workspace::new_id;
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, Selectable, Sizable, checkbox::Checkbox, input::Input, v_flex,
};
use gpui_kit::{PathPromptOptions, Render, div, px};

struct ConnectionForm {
    choosing_engine: bool,
    engine_category: EngineCategory,
    engine_search: Entity<InputState>,
    _engine_search_changes: Subscription,
    config: ConnectionConfig,
    name: Entity<InputState>,
    database: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    error: String,
    status: String,
    testing: bool,
    loading_password: bool,
    password_edited: bool,
    password_target: ConnectionConfig,
    credentials: std::sync::Arc<dyn crate::credentials::PasswordStore>,
    _password_changes: Subscription,
    _password_task: Option<Task<()>>,
    _test: Option<Task<()>>,
    _browse: Option<Task<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EngineCategory {
    All,
    Files,
    Servers,
    Analytical,
}

impl EngineCategory {
    fn contains(self, engine: Engine) -> bool {
        match self {
            Self::All => true,
            Self::Files => matches!(engine, Engine::SQLite | Engine::DuckDB),
            Self::Servers => matches!(engine, Engine::Postgres | Engine::MySql),
            Self::Analytical => engine == Engine::DuckDB,
        }
    }
}

fn engine_description(engine: Engine) -> &'static str {
    match engine {
        Engine::SQLite => "Local database file",
        Engine::DuckDB => "Embedded analytics database",
        Engine::Postgres => "PostgreSQL server",
        Engine::MySql => "MySQL server",
    }
}

fn engine_index(engine: Engine) -> usize {
    match engine {
        Engine::SQLite => 0,
        Engine::DuckDB => 1,
        Engine::Postgres => 2,
        Engine::MySql => 3,
    }
}

impl ConnectionForm {
    fn new(
        config: ConnectionConfig,
        credentials: std::sync::Arc<dyn crate::credentials::PasswordStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = |value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        let engine_search = input(String::new(), window, cx);
        let engine_search_changes =
            cx.subscribe(&engine_search, |_, _, _: &InputEvent, cx| cx.notify());
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .default_value(config.password.clone())
        });
        let changes = cx.subscribe(&password, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change)
                && state.read(cx).value().as_str() != this.config.password
            {
                this.password_edited = true;
                cx.notify();
            }
        });
        let mut password_target = config.clone();
        password_target.password.clear();
        Self {
            choosing_engine: false,
            engine_category: EngineCategory::All,
            engine_search,
            _engine_search_changes: engine_search_changes,
            name: input(config.name.clone(), window, cx),
            database: input(config.database.clone(), window, cx),
            host: input(config.host.clone(), window, cx),
            port: input(config.port.to_string(), window, cx),
            user: input(config.user.clone(), window, cx),
            password,
            config,
            error: String::new(),
            status: String::new(),
            testing: false,
            loading_password: false,
            password_edited: false,
            password_target,
            credentials,
            _password_changes: changes,
            _password_task: None,
            _test: None,
            _browse: None,
        }
    }
    fn visible_engines(&self, cx: &App) -> Vec<Engine> {
        let search = self.engine_search.read(cx).value().trim().to_lowercase();
        Engine::ALL
            .into_iter()
            .filter(|engine| {
                self.engine_category.contains(*engine)
                    && format!("{} {}", engine.name(), engine_description(*engine))
                        .to_lowercase()
                        .contains(&search)
            })
            .collect()
    }

    fn select_engine(&mut self, engine: Engine, window: &mut Window, cx: &mut Context<Self>) {
        if self.config.engine != engine {
            self.config.engine = engine;
            let port = if engine == Engine::MySql { 3306 } else { 5432 };
            self.port.update(cx, |state, cx| {
                state.set_value(port.to_string(), window, cx)
            });
            self.error.clear();
            self.status.clear();
        }
        cx.notify();
    }

    fn render_engine_picker(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let categories = v_flex().w(px(140.)).flex_shrink_0().gap_1().children(
            [
                (EngineCategory::All, "All databases"),
                (EngineCategory::Files, "Local files"),
                (EngineCategory::Servers, "Servers"),
                (EngineCategory::Analytical, "Analytical"),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (category, label))| {
                Button::new(("engine-category", index))
                    .ghost()
                    .small()
                    .justify_start()
                    .label(label)
                    .selected(self.engine_category == category)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.engine_category = category;
                        cx.notify();
                    }))
            }),
        );
        let engines = self.visible_engines(cx);
        let cards = v_flex().gap_3().children(engines.chunks(2).map(|pair| {
            h_flex()
                .gap_3()
                .children(pair.iter().copied().map(|engine| {
                    let selected = self.config.engine == engine;
                    Button::new(("engine", engine_index(engine)))
                        .ghost()
                        .w(px(216.))
                        .h(px(104.))
                        .p_3()
                        .border_1()
                        .rounded(px(6.))
                        .border_color(if selected {
                            cx.theme().ring
                        } else {
                            cx.theme().border
                        })
                        .selected(selected)
                        .accessibility_label(format!(
                            "Select {}. {}",
                            engine.name(),
                            engine_description(engine)
                        ))
                        .child(
                            v_flex()
                                .w_full()
                                .items_start()
                                .gap_2()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .justify_between()
                                        .child(Icon::new(IconName::Database).size(px(28.)))
                                        .when(selected, |row| {
                                            row.child(Icon::new(IconName::Check).small())
                                        }),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child(engine.name()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(engine_description(engine)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_engine(engine, window, cx)
                        }))
                }))
        }));
        let catalog = v_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(
                Input::new(&self.engine_search)
                    .prefix(IconName::Search)
                    .cleanable(true)
                    .aria_label("Search supported databases"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} supported databases", engines.len())),
            )
            .child(cards)
            .when(engines.is_empty(), |column| {
                column.child(
                    div()
                        .id("no-engine-matches")
                        .p_4()
                        .text_sm()
                        .child("No supported databases match your search.")
                        .test_support(),
                )
            });
        h_flex()
            .id("database-engine-picker")
            .gap_5()
            .h(px(300.))
            .child(categories)
            .child(catalog)
            .test_support()
            .into_any_element()
    }
    fn restore_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.config.engine, Engine::Postgres | Engine::MySql)
            || !self.config.remember_password
            || !self.config.password.is_empty()
        {
            return;
        }
        self.loading_password = true;
        let store = self.credentials.clone();
        let config = self.config.clone();
        let work = cx
            .background_executor()
            .spawn(async move { store.load(&config) });
        self._password_task = Some(cx.spawn_in(window, async move |form, cx| {
            let result = work.await;
            let _ = form.update_in(cx, |this, window, cx| {
                this.loading_password = false;
                match result {
                    Ok(Some(password)) => {
                        this.config.password = password.clone();
                        this.password
                            .update(cx, |state, cx| state.set_value(password, window, cx));
                        this.status = "Password restored from the OS credential store.".into();
                    }
                    Ok(None) => {}
                    Err(error) => this.error = error,
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn config(&self, cx: &App) -> Result<ConnectionConfig, String> {
        let mut config = self.config.clone();
        config.name = self.name.read(cx).value().trim().to_string();
        config.database = self.database.read(cx).value().trim().to_string();
        config.host = self.host.read(cx).value().trim().to_string();
        config.user = self.user.read(cx).value().to_string();
        config.password = self.password.read(cx).value().to_string();
        if config.name.is_empty() {
            return Err("A connection name is required.".into());
        }
        if config.database.is_empty() {
            return Err("Choose a database file or enter a database name.".into());
        }
        if matches!(config.engine, Engine::SQLite | Engine::DuckDB) && config.database != ":memory:"
        {
            let path = std::path::PathBuf::from(&config.database);
            let absolute = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|_| "Cannot resolve the database path. Choose an absolute file path.")?
                    .join(path)
            };
            if !absolute.is_absolute() {
                return Err(
                    "Use a complete absolute database path or a path relative to this directory."
                        .into(),
                );
            }
            // Persist the resolved location so a later launch directory cannot
            // silently select a different database for this profile.
            config.database = absolute
                .to_str()
                .ok_or("The database path must be valid Unicode.")?
                .to_owned();
        }
        if matches!(config.engine, Engine::Postgres | Engine::MySql) {
            config.port = self
                .port
                .read(cx)
                .value()
                .parse::<u16>()
                .map_err(|_| "Port must be between 1 and 65535.".to_string())?;
            if config.port == 0 || config.host.is_empty() {
                return Err("Enter a host and valid port.".into());
            }
            if !self.password_edited
                && !self.config.password.is_empty()
                && !crate::credentials::same_endpoint(&config, &self.password_target)
            {
                return Err("Server details changed. Re-enter the password for this target before connecting.".into());
            }
        } else {
            config.password.clear();
            config.remember_password = false;
        }
        Ok(config)
    }
    fn test_connection(&mut self, cx: &mut Context<Self>) {
        if self.testing || self.loading_password {
            return;
        }
        let config = match self.config(cx) {
            Ok(config) => config,
            Err(error) => {
                self.error = error;
                cx.notify();
                return;
            }
        };
        self.testing = true;
        self.error.clear();
        self.status = "Testing connection…".into();
        let work = cx.background_executor().spawn(async move {
            let session = DatabaseSession::connect(&config)?;
            session.catalog().map(|objects| objects.len())
        });
        self._test = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |this, cx| {
                this.testing = false;
                match result {
                    Ok(count) => this.status = format!("Connection succeeded · {count} objects"),
                    Err(error) => {
                        this.status.clear();
                        this.error = format!("Connection failed: {error}");
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a database file".into()),
        });
        self._browse = Some(cx.spawn_in(window, async move |view, cx| {
            let selected = match prompt.await {
                Ok(Ok(Some(paths))) => {
                    Ok(paths.first().map(|path| path.to_string_lossy().to_string()))
                }
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = cx.update_root(|_, window, cx| {
                view.update(cx, |this, cx| {
                    match selected {
                        Ok(Some(value)) => this
                            .database
                            .update(cx, |state, cx| state.set_value(value, window, cx)),
                        Ok(None) => {}
                        Err(error) => this.error = format!("Could not choose a file: {error}"),
                    }
                    cx.notify();
                })
            });
        }));
    }
}

impl Render for ConnectionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.choosing_engine {
            return self.render_engine_picker(cx);
        }
        let local = matches!(self.config.engine, Engine::SQLite | Engine::DuckDB);
        let testing = self.testing || self.loading_password;
        let field = |label: &'static str, input: &Entity<InputState>| field(label, input, testing);
        let engines = h_flex()
            .gap_3()
            .p_3()
            .rounded(px(6.))
            .bg(cx.theme().muted)
            .child(Icon::new(IconName::Database).size(px(28.)))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(self.config.engine.name()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(engine_description(self.config.engine)),
                    ),
            );
        let server_address = h_flex()
            .gap_3()
            .child(div().flex_1().child(field("Host", &self.host)))
            .child(div().w(px(100.)).child(field("Port", &self.port)));
        let credentials = h_flex()
            .gap_3()
            .child(div().flex_1().child(field("User", &self.user)))
            .child(div().flex_1().child(field("Password", &self.password)));
        let tls = Checkbox::new("connection-tls")
            .disabled(testing)
            .label("Require TLS")
            .checked(self.config.tls)
            .on_click(cx.listener(|this, value, _, cx| {
                this.config.tls = *value;
                cx.notify();
            }));
        let read_only = Checkbox::new("connection-read-only")
            .disabled(testing)
            .label("Read-only connection")
            .checked(self.config.read_only)
            .on_click(cx.listener(|this, value, _, cx| {
                this.config.read_only = *value;
                cx.notify();
            }));
        let explanation = div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(if local {
                "Read-only connections protect existing data. Choose a file or use :memory: for a temporary database."
            } else {
                "Passwords never enter workspace files. Remembered passwords use your OS credential store. Uncheck Remember to remove the saved password on a successful connection."
            });
        v_flex()
            .gap_4()
            .when(!self.status.is_empty(), |element| {
                element.child(
                    div()
                        .id("connection-test-status")
                        .role(gpui_kit::Role::Status)
                        .aria_label(self.status.clone())
                        .text_sm()
                        .text_color(cx.theme().success)
                        .child(self.status.clone()),
                )
            })
            .when(!self.error.is_empty(), |element| {
                element.child(
                    div()
                        .id("connection-form-error")
                        .role(gpui_kit::Role::Alert)
                        .aria_label(self.error.clone())
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(self.error.clone()),
                )
            })
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_xs().child("Database engine"))
                    .child(engines),
            )
            .child(field("Connection name", &self.name))
            .child(field(
                if local {
                    "Database file (:memory: for scratch)"
                } else {
                    "Database"
                },
                &self.database,
            ))
            .when(local, |element| {
                element.child(
                    Button::new("browse-database")
                        .ghost()
                        .small()
                        .disabled(testing)
                        .label("Choose existing file…")
                        .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                )
            })
            .when(!local, |element| {
                element
                    .child(server_address)
                    .child(credentials)
                    .child(tls)
                    .child(
                        Checkbox::new("remember-password")
                            .label("Remember password on this device")
                            .checked(self.config.remember_password)
                            .disabled(testing)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.config.remember_password = *checked;
                                cx.notify();
                            })),
                    )
            })
            .child(read_only)
            .child(explanation)
            .into_any_element()
    }
}
fn field(label: &'static str, input: &Entity<InputState>, disabled: bool) -> gpui_kit::AnyElement {
    v_flex()
        .gap_1()
        .child(div().text_xs().child(label))
        .child(Input::new(input).disabled(disabled).aria_label(label))
        .into_any_element()
}

impl Studio {
    fn delete_profile(&mut self, id: String, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting {
            return;
        }
        let Some(profile) = self
            .workspace
            .connections
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };
        self.connecting = true;
        let credentials = self.credentials.clone();
        let work = cx
            .background_executor()
            .spawn(async move { credentials.remove(&profile) });
        self._connection_task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |this, cx| {
                this.connecting = false;
                match result {
                    Ok(()) => {
                        this.workspace.connections.retain(|profile| profile.id != id);
                        this.notice = "Saved profile and its stored password removed. Connected sessions remain available until disconnected.".into();
                        this.persist();
                    }
                    Err(error) => this.notice = format!("{error} The profile was kept; retry after unlocking the credential store."),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save_password(&mut self, config: ConnectionConfig, cx: &mut Context<Self>) {
        self.connecting = true;
        let credentials = self.credentials.clone();
        let request = config.clone();
        let work = cx
            .background_executor()
            .spawn(async move { credentials.save(&request) });
        self._connection_task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |this, cx| {
                this.connecting = false;
                match result {
                    Ok(()) => {
                        if let Some(profile) = this.workspace.connections.iter_mut()
                            .find(|profile| profile.id == config.id)
                        {
                            profile.remember_password = config.remember_password
                                && !config.password.is_empty()
                                && matches!(config.engine, Engine::Postgres | Engine::MySql);
                            this.notice = if profile.remember_password {
                                "Connected. Password saved in the OS credential store."
                            } else {
                                "Connected. Password remains session-only; any saved password was removed."
                            }.into();
                            this.persist();
                        }
                    }
                    Err(error) => this.notice = format!("Connected, but {error} Password remembering is disabled for this profile."),
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn remove_profile(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.any_running() || self.connecting {
            return;
        }
        let view = cx.entity().downgrade();
        window.open_dialog(cx,move |dialog,_,cx| {
            let view=view.clone();let id=id.clone();
            dialogs::frame(dialog, "Connection settings", "Remove this profile?", "Remove its settings and securely stored password.", 480., cx)
                .child("This removes the saved settings and the password in the OS credential store. Database files, server data, open query drafts, and currently connected sessions are kept.")
                .footer(h_flex().gap_2().justify_end()
                    .child(Button::new("cancel-remove-profile").label("Cancel").on_click(|_,window,cx|window.close_dialog(cx)))
                    .child(Button::new("confirm-remove-profile").danger().label("Remove profile").on_click(move |_,window,cx| {
                        if let Some(view)=view.upgrade() {view.update(cx,|this,cx| {
                            this.delete_profile(id.clone(), cx);
                        });}
                        window.close_dialog(cx);
                    })))
        });
    }
    pub(super) fn connection_dialog(
        &mut self,
        profile: Option<ConnectionConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.connecting || self.any_running() {
            return;
        }
        let choosing_engine = profile.is_none();
        let mut config = profile.unwrap_or_default();
        if config.id.is_empty() {
            config.id = new_id("connection");
        }
        let reconnecting = self
            .connections
            .iter()
            .any(|connection| connection.config.id == config.id);
        let credentials = self.credentials.clone();
        let form = cx.new(|cx| ConnectionForm::new(config, credentials, window, cx));
        form.update(cx, |form, cx| {
            form.choosing_engine = choosing_engine;
            form.restore_password(window, cx);
        });
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let back_form = form.clone();
            let next_form = form.clone();
            let choosing_engine = form.read(cx).choosing_engine;
            let selection_visible = form.read(cx).visible_engines(cx).contains(&form.read(cx).config.engine);
            let title = if choosing_engine { "Select your database".to_owned() } else { format!("{} connection", form.read(cx).config.engine.name()) };
            let description = if choosing_engine { "Choose a database, then configure its connection." } else { "Configure connection details and access mode." };
            let test_form = form.clone();
            let connect_form = form.clone();
            let connect_view = view.clone();
            let testing=form.read(cx).testing || form.read(cx).loading_password;
            dialogs::frame(dialog, "Connections", title, description, if choosing_engine { 720. } else { 580. }, cx)
                .child(form.clone())
                .when(reconnecting && !choosing_engine,|dialog|dialog.child("Reconnecting closes the existing session and rolls back any uncommitted transaction. Open query drafts are kept."))
                .footer(
                    h_flex()
                        .gap_2()
                        .w_full()
                        .when(!choosing_engine, |footer| footer.child(
                            Button::new("back-to-engines").ghost().label("Back").disabled(testing)
                                .on_click(move |_, _, cx| back_form.update(cx, |form, cx| { form.choosing_engine = true; cx.notify(); })),
                        ))
                        .child(div().flex_1())
                        .child(
                            Button::new("cancel-connect")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .when(choosing_engine, |footer| footer.child(
                            Button::new("choose-engine-next").primary().label("Next").disabled(testing || !selection_visible)
                                .on_click(move |_, _, cx| next_form.update(cx, |form, cx| {
                                    if form.visible_engines(cx).contains(&form.config.engine) && !form.testing && !form.loading_password {
                                        form.choosing_engine = false;
                                        cx.notify();
                                    }
                                })),
                        ))
                        .when(!choosing_engine, |footer| footer.child(
                            Button::new("test-connection")
                                .label("Test connection")
                                .disabled(testing)
                                .on_click(move |_, _, cx|test_form.update(cx,|form,cx|form.test_connection(cx))),
                        )
                        .child(
                            Button::new("connect-database")
                                .primary()
                                .label(if reconnecting {"Reconnect"}else{"Connect"})
                                .disabled(testing)
                                .on_click(move |_, window, cx| {
                                    if connect_form.read(cx).testing || connect_form.read(cx).loading_password {return;}
                                    if let Some(view) = connect_view.upgrade() {
                                        let config = connect_form.read(cx).config(cx);
                                        view.update(cx, |this, cx| match config {
                                            Ok(config) => {
                                                this.start_connection(config, false, cx);
                                                window.close_dialog(cx);
                                            }
                                            Err(error) => {
                                                connect_form.update(cx, |form, cx| {
                                                    form.error = error.clone();
                                                    cx.notify();
                                                });
                                                this.notice = error;
                                                cx.notify();
                                            }
                                        });
                                    }
                                }),
                        )),
                )
        });
    }
    pub(super) fn scratch(&mut self, cx: &mut Context<Self>) {
        let config = ConnectionConfig {
            id: new_id("scratch"),
            name: "SQLite scratch".into(),
            engine: Engine::SQLite,
            database: ":memory:".into(),
            read_only: false,
            ..Default::default()
        };
        self.start_connection(config, false, cx);
    }
    pub(super) fn start_connection(
        &mut self,
        config: ConnectionConfig,
        test_only: bool,
        cx: &mut Context<Self>,
    ) {
        if self.connecting || self.any_running() {
            return;
        }
        self.connecting = true;
        self.notice = format!(
            "{} {}…",
            if test_only {
                "Testing"
            } else {
                "Connecting to"
            },
            config.name
        );
        let request = config.clone();
        let work = cx.background_executor().spawn(async move {
            let session = DatabaseSession::connect(&request)?;
            let catalog = session.catalog()?;
            Ok::<_, crate::database::DbError>((session, catalog))
        });
        self._connection_task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |this, cx| {
                this.connecting = false;
                match result {
                    Ok((session, catalog)) => {
                        if test_only {
                            this.notice = format!(
                                "Connection to {} succeeded ({} objects).",
                                config.name,
                                catalog.len()
                            );
                        } else {
                            this.connections.retain(|c| c.config.id != config.id);
                            this.connections.push(LiveConnection {
                                config: config.clone(),
                                session,
                                catalog,
                            });
                            this.active_connection = Some(config.id.clone());
                            this.notice = format!("Connected to {}.", config.name);
                            // Scratch databases are ephemeral and must never be restored as file profiles.
                            if config.database != ":memory:" {
                                this.workspace.connections.retain(|c| c.id != config.id);
                                let mut saved = config.clone();
                                saved.password.clear();
                                // Commit profile metadata before touching the credential store.
                                // Until the native write succeeds, never claim a password is remembered.
                                saved.remember_password = false;
                                this.workspace.connections.push(saved);
                                if this.persist() {
                                    this.save_password(config.clone(), cx);
                                } else {
                                    this.notice.push_str(
                                        " Password was not saved to the credential store.",
                                    );
                                }
                            }
                            this.clear_schema();
                            this.rebuild_tree(cx);
                        }
                    }
                    Err(error) => this.notice = format!("Connection failed: {error}"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn select_connection(&mut self, id: String, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting {
            return;
        }
        self.active_connection = Some(id);
        self.clear_schema();
        self.rebuild_tree(cx);
    }
    pub(super) fn disconnect(&mut self, cx: &mut Context<Self>) {
        if self.any_running() || self.connecting {
            return;
        }
        if let Some(id) = self.active_connection.take() {
            self.connections.retain(|c| c.config.id != id);
        }
        self.active_connection = self.connections.first().map(|c| c.config.id.clone());
        self.clear_schema();
        self.rebuild_tree(cx);
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    use crate::credentials::{MemoryPasswordStore, PasswordStore};
    use gpui_kit::TestAppContext;
    use std::sync::Arc;

    #[gpui_kit::test]
    fn engine_picker_filters_real_drivers_and_preserves_entered_settings(cx: &mut TestAppContext) {
        let (handle, _) = super::super::tests::open_studio(cx, 900., 620.);
        cx.update_window(handle, |_, window, cx| {
            let mut config = profile();
            config.password.clear();
            let form = cx.new(|cx| {
                ConnectionForm::new(config, Arc::new(MemoryPasswordStore::default()), window, cx)
            });
            form.update(cx, |form, cx| {
                assert_eq!(form.visible_engines(cx), Engine::ALL);
                form.engine_category = EngineCategory::Servers;
                assert_eq!(
                    form.visible_engines(cx),
                    vec![Engine::Postgres, Engine::MySql]
                );
                form.engine_search
                    .update(cx, |input, cx| input.set_value("mysql", window, cx));
                assert_eq!(form.visible_engines(cx), vec![Engine::MySql]);
                form.select_engine(Engine::MySql, window, cx);
                form.choosing_engine = false;
                let settings = form.config(cx).expect("Configured target");
                assert_eq!(settings.port, 3306);
                assert_eq!(settings.name, "Test server");
                assert_eq!(settings.database, "fixture");
                assert!(settings.password.is_empty());
                assert!(settings.read_only && settings.tls);
                form.choosing_engine = true;
                let restored = form.config(cx).expect("Back preserves settings");
                assert_eq!(restored.name, settings.name);
                assert_eq!(restored.database, settings.database);
                assert_eq!(restored.password, settings.password);
                assert_eq!(restored.port, settings.port);
                assert_eq!(restored.engine, settings.engine);
                form.engine_search
                    .update(cx, |input, cx| input.set_value("unsupported", window, cx));
                assert!(form.visible_engines(cx).is_empty());
            });
        })
        .expect("Engine picker filtering and form state");
    }

    fn profile() -> ConnectionConfig {
        ConnectionConfig {
            id: new_id("form-credential-test"),
            engine: Engine::Postgres,
            name: "Test server".into(),
            database: "fixture".into(),
            password: "synthetic-form-password".into(),
            ..Default::default()
        }
    }

    #[gpui_kit::test]
    fn restored_password_requires_reentry_after_endpoint_change(cx: &mut TestAppContext) {
        let (handle, _) = super::super::tests::open_studio(cx, 900., 620.);
        let store = Arc::new(MemoryPasswordStore::default());
        let mut config = profile();
        store.save(&config).expect("Synthetic credential");
        config.password.clear();
        let form = cx
            .update_window(handle, |_, window, cx| {
                let form = cx.new(|cx| ConnectionForm::new(config, store, window, cx));
                form.update(cx, |form, cx| form.restore_password(window, cx));
                form
            })
            .expect("Form");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            form.update(cx, |form, cx| {
                assert!(!form.loading_password);
                assert!(!form.password_edited);
                assert_eq!(
                    form.config(cx).expect("Restored").password,
                    "synthetic-form-password"
                );
                form.host
                    .update(cx, |state, cx| state.set_value("other.invalid", window, cx));
                assert!(form.config(cx).is_err());
                form.password.update(cx, |state, cx| {
                    state.set_value("replacement", window, cx);
                    // Programmatic set_value is silent; typing emits Change.
                    cx.emit(InputEvent::Change);
                });
            });
        })
        .expect("Change endpoint");
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert_eq!(
                form.read(cx)
                    .config(cx)
                    .expect("Explicit replacement")
                    .password,
                "replacement"
            );
        })
        .expect("Validate replacement");
    }

    #[gpui_kit::test]
    fn credential_save_and_profile_removal_complete_asynchronously(cx: &mut TestAppContext) {
        let (handle, studio) = super::super::tests::open_studio(cx, 900., 620.);
        let config = profile();
        let store = Arc::new(MemoryPasswordStore::default());
        cx.update_window(handle, |_, _, cx| {
            studio.update(cx, |studio, cx| {
                studio.credentials = store.clone();
                let mut metadata = config.clone();
                metadata.password.clear();
                metadata.remember_password = false;
                studio.workspace.connections.push(metadata);
                studio.save_password(config.clone(), cx);
            });
        })
        .expect("Save password");
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            studio.update(cx, |studio, cx| {
                assert!(!studio.connecting);
                assert!(
                    studio
                        .workspace
                        .connections
                        .last()
                        .expect("Profile")
                        .remember_password
                );
                assert!(
                    !serde_json::to_string(&studio.workspace)
                        .expect("Workspace metadata")
                        .contains(&config.password)
                );
                assert_eq!(
                    store.load(&config).expect("Stored password"),
                    Some(config.password.clone())
                );
                studio.delete_profile(config.id.clone(), cx);
            });
        })
        .expect("Remove profile");
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert!(!studio.read(cx).connecting);
            assert!(
                studio
                    .read(cx)
                    .workspace
                    .connections
                    .iter()
                    .all(|profile| profile.id != config.id)
            );
            assert!(store.load(&config).expect("Removed credential").is_none());
        })
        .expect("Removal completed");
    }

    #[gpui_kit::test]
    fn successful_connection_finishes_followup_credential_task(cx: &mut TestAppContext) {
        let (handle, studio) = super::super::tests::open_studio(cx, 900., 620.);
        let path = std::env::temp_dir().join(format!("{}.sqlite", new_id("connection-test")));
        let config = ConnectionConfig {
            id: new_id("connection-test"),
            database: path.to_string_lossy().into_owned(),
            read_only: false,
            ..Default::default()
        };
        cx.update_window(handle, |_, _, cx| {
            studio.update(cx, |studio, cx| {
                studio.start_connection(config.clone(), false, cx)
            });
        })
        .expect("Connect local fixture");
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            studio.update(cx, |studio, _| {
                assert!(!studio.connecting, "Credential followup must complete");
                assert_eq!(studio.connections.len(), 1);
                assert!(
                    !studio
                        .workspace
                        .connections
                        .last()
                        .expect("Saved profile")
                        .remember_password
                );
                assert!(studio.notice.contains("saved password was removed"));
                studio.connections.clear();
            });
        })
        .expect("Completed followup");
        std::fs::remove_file(path).expect("Remove synthetic database");
    }

    struct UnavailableStore;
    impl PasswordStore for UnavailableStore {
        fn load(&self, _: &ConnectionConfig) -> Result<Option<String>, String> {
            Err("Credential store locked".into())
        }
        fn save(&self, _: &ConnectionConfig) -> Result<(), String> {
            Err("Credential store locked".into())
        }
        fn remove(&self, _: &ConnectionConfig) -> Result<(), String> {
            Err("Credential store locked".into())
        }
    }

    #[gpui_kit::test]
    fn unavailable_store_keeps_profile_and_never_claims_password_is_saved(cx: &mut TestAppContext) {
        let (handle, studio) = super::super::tests::open_studio(cx, 900., 620.);
        let config = profile();
        cx.update_window(handle, |_, _, cx| {
            studio.update(cx, |studio, cx| {
                studio.credentials = Arc::new(UnavailableStore);
                let mut metadata = config.clone();
                metadata.password.clear();
                metadata.remember_password = false;
                studio.workspace.connections.push(metadata);
                studio.save_password(config.clone(), cx);
            });
        })
        .expect("Failed save");
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            studio.update(cx, |studio, cx| {
                assert!(!studio.connecting);
                assert!(
                    !studio
                        .workspace
                        .connections
                        .last()
                        .expect("Profile")
                        .remember_password
                );
                assert!(studio.notice.contains("locked"));
                studio.delete_profile(config.id.clone(), cx);
            });
        })
        .expect("Failed removal");
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert!(!studio.read(cx).connecting);
            assert!(
                studio
                    .read(cx)
                    .workspace
                    .connections
                    .iter()
                    .any(|profile| profile.id == config.id)
            );
            assert!(studio.read(cx).notice.contains("locked"));
        })
        .expect("Profile retained");
    }
}
