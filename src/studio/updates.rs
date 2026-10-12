//! Update checks and verified staging never perform network or disk work on the UI thread.

use super::*;
use crate::updater::{PreparedUpdate, Release};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, v_flex};
use gpui_kit::{Render, WeakEntity, div, px};
use std::{sync::Arc, time::Duration};

pub(super) enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(Release),
    Downloading(Release),
    Ready(Release, Arc<PreparedUpdate>),
    Error(String, Option<Release>),
}

impl UpdateState {
    pub(super) fn toolbar_label(&self) -> &'static str {
        match self {
            Self::Available(_) => "Update available",
            Self::Downloading(_) => "Downloading…",
            Self::Ready(_, _) => "Restart to update",
            _ => "Updates",
        }
    }

    fn can_check(&self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Current | Self::Available(_) | Self::Error(_, _)
        )
    }

    fn downloadable_release(&self) -> Option<&Release> {
        match self {
            Self::Available(release) | Self::Error(_, Some(release)) => Some(release),
            _ => None,
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Idle => "Check for the latest stable release.".into(),
            Self::Checking => "Checking for updates…".into(),
            Self::Current => "You are running the latest stable release.".into(),
            Self::Available(release) => format!("Version {} is available.", release.version),
            Self::Downloading(release) => {
                format!("Downloading and verifying version {}…", release.version)
            }
            Self::Ready(release, _) => format!(
                "Version {} is verified and ready. Restart to install it.",
                release.version
            ),
            Self::Error(error, _) => format!("Update failed: {error}"),
        }
    }

    fn release(&self) -> Option<&Release> {
        match self {
            Self::Available(release) | Self::Downloading(release) | Self::Ready(release, _) => {
                Some(release)
            }
            Self::Error(_, release) => release.as_ref(),
            _ => None,
        }
    }
}

impl Studio {
    pub(super) fn start_update_timer(&mut self, cx: &mut Context<Self>) {
        // Test executors advance fake timers until quiescent; a recurring timer must
        // not start real release requests or keep native view tests alive forever.
        if cfg!(test) || self._update_timer.is_some() {
            return;
        }
        let executor = cx.background_executor().clone();
        self._update_timer = Some(cx.spawn(async move |view, cx| {
            executor.timer(Duration::from_secs(5)).await;
            loop {
                if view
                    .update(cx, |this, cx| {
                        if this.workspace.preferences.automatic_updates {
                            this.check_updates(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
                executor.timer(Duration::from_secs(6 * 60 * 60)).await;
            }
        }));
    }

    fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.installing_update || !self.update_state.can_check() {
            return;
        }
        self.update_state = UpdateState::Checking;
        let executor = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |view, cx| {
            let result = executor.spawn(async { crate::updater::check() }).await;
            let _ = view.update(cx, |this, cx| {
                this.update_state = match result {
                    Ok(Some(release)) => UpdateState::Available(release),
                    Ok(None) => UpdateState::Current,
                    Err(error) => UpdateState::Error(error.to_string(), None),
                };
                if this.workspace.preferences.automatic_updates {
                    this.download_update(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn download_update(&mut self, cx: &mut Context<Self>) {
        if self.installing_update {
            return;
        }
        let Some(release) = self.update_state.downloadable_release().cloned() else {
            return;
        };
        self.update_state = UpdateState::Downloading(release.clone());
        let executor = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |view, cx| {
            let request = release.clone();
            let result = executor
                .spawn(async move { crate::updater::download(&request) })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.update_state = match result {
                    Ok(prepared) => UpdateState::Ready(release, Arc::new(prepared)),
                    Err(error) => UpdateState::Error(error.to_string(), Some(release)),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn update_restart_blocker(&self) -> Option<&'static str> {
        if self.installing_update
            || self.any_running()
            || self.connecting
            || [
                &self._file_task,
                &self._catalog_task,
                &self._connection_task,
            ]
            .into_iter()
            .any(|task| task.as_ref().is_some_and(|task| !task.is_ready()))
        {
            return Some("Wait for database and file operations to finish before restarting.");
        }
        if !self.connections.is_empty() {
            return Some(
                "Finish or roll back transactions, then disconnect databases before restarting.",
            );
        }
        if self
            .tabs
            .iter()
            .any(|tab| draft_requires_saving(tab.dirty, tab.saved_id.is_some(), &tab.editor_source))
        {
            return Some(
                "Save all nonempty drafts to the workspace, or close them, before restarting.",
            );
        }
        if self.storage_error.is_some() {
            return Some(
                "Workspace storage is unavailable. Resolve the storage error before restarting.",
            );
        }
        None
    }

    fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reason) = self.update_restart_blocker() {
            self.notice = reason.into();
            cx.notify();
            return;
        }
        let UpdateState::Ready(release, prepared) = &self.update_state else {
            return;
        };
        let version = release.version.clone();
        let prepared = Arc::clone(prepared);
        if !self.persist() {
            cx.notify();
            return;
        }
        // Block the whole workspace while preparing the helper, so no fresh query,
        // file operation or editor change can race the last persistence check.
        self.installing_update = true;
        self.connecting = true;
        window.close_dialog(cx);
        window.blur(cx);
        let executor = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |view, cx| {
            let result = executor
                .spawn(async move {
                    crate::updater::install::launch_install(&prepared.staged_root, &version)
                })
                .await;
            let _ = view.update(cx, |this, cx| match result {
                Ok(()) => cx.quit(),
                Err(error) => {
                    this.installing_update = false;
                    this.connecting = false;
                    this.notice =
                        format!("Could not start the update: {error}. Open Updates to retry.");
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn open_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let studio = entity.downgrade();
        let panel = cx.new(|cx| {
            let subscription = cx.observe(&entity, |_, _, cx| cx.notify());
            UpdatePanel {
                studio,
                _subscription: subscription,
            }
        });
        window.open_dialog(cx, move |dialog, _, cx| {
            dialogs::frame(
                dialog,
                "Relix",
                "Application updates",
                "Stable releases are downloaded and verified before installation.",
                520.,
                cx,
            )
            .child(panel.clone())
        });
    }
}

pub(super) fn draft_requires_saving(dirty: bool, saved: bool, source: &str) -> bool {
    dirty || (!saved && !source.trim().is_empty())
}

struct UpdatePanel {
    studio: WeakEntity<Studio>,
    _subscription: Subscription,
}

impl Render for UpdatePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(studio) = self.studio.upgrade() else {
            return div().into_any_element();
        };
        let state = studio.read(cx);
        let auto = state.workspace.preferences.automatic_updates;
        let blocker = state.update_restart_blocker();
        let notes = state
            .update_state
            .release()
            .map(|release| release.notes.clone())
            .unwrap_or_default();
        v_flex().gap_3()
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("Installed version {}", env!("CARGO_PKG_VERSION"))))
            .child(div().id("update-status").role(gpui_kit::Role::Status).child(state.update_state.description()))
            .when(!notes.is_empty(), |column| column.child(div().id("update-release-notes").max_h(px(180.)).overflow_y_scroll().text_sm().child(notes)))
            .child(Button::new("automatic-updates").ghost().small()
                .label(if auto { "Automatic updates: on" } else { "Automatic updates: off" })
                .tooltip("Check at startup and every six hours; download verified updates and ask before restarting")
                .on_click(cx.listener(|this, _, _, cx| {
                    let _ = this.studio.update(cx, |studio, cx| {
                        let previous = studio.workspace.preferences.automatic_updates;
                        studio.workspace.preferences.automatic_updates = !previous;
                        if !studio.persist() { studio.workspace.preferences.automatic_updates = previous; }
                        else if !previous { studio.check_updates(cx); }
                        cx.notify();
                    });
                })))
            .when(matches!(state.update_state, UpdateState::Ready(_, _)) && blocker.is_some(), |column| column.child(div().text_xs().child(blocker.unwrap_or_default())))
            .child(h_flex().gap_2().justify_end()
                .child(Button::new("check-updates").label("Check for updates").disabled(!state.update_state.can_check())
                    .on_click(cx.listener(|this, _, _, cx| { let _ = this.studio.update(cx, |studio, cx| studio.check_updates(cx)); })))
                .when(state.update_state.downloadable_release().is_some(), |row| row.child(Button::new("download-update").primary().label("Download update")
                    .on_click(cx.listener(|this, _, _, cx| { let _ = this.studio.update(cx, |studio, cx| studio.download_update(cx)); }))))
                .when(matches!(state.update_state, UpdateState::Ready(_, _)), |row| row.child(Button::new("install-update").primary().label("Restart and install").disabled(blocker.is_some())
                    .on_click(cx.listener(|this, _, window, cx| { let _ = this.studio.update(cx, |studio, cx| studio.install_update(window, cx)); }))))
                .child(Button::new("close-updates").label("Close").on_click(|_, window, cx| window.close_dialog(cx))))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_only_drafts_require_saving_even_without_edits() {
        assert!(draft_requires_saving(false, false, "SELECT 1;"));
        assert!(draft_requires_saving(true, true, "SELECT 2;"));
        assert!(!draft_requires_saving(false, true, "SELECT 1;"));
        assert!(!draft_requires_saving(false, false, " \n\t"));
    }

    #[test]
    fn checks_cannot_start_while_a_check_is_pending() {
        assert!(!UpdateState::Checking.can_check());
        assert!(UpdateState::Idle.can_check());
        assert!(UpdateState::Error("offline".into(), None).can_check());
        assert!(UpdateState::Current.downloadable_release().is_none());
    }
}
