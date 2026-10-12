use super::*;
use gpui_kit::component::{ActiveTheme, input::Input, v_flex};
use gpui_kit::{Render, div};

struct RenameForm {
    input: Entity<InputState>,
    error: Option<String>,
}

impl Render for RenameForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(Input::new(&self.input).aria_label("Query title"))
            .child("Use a title of 1 to 128 characters (at most 512 bytes).")
            .when_some(self.error.clone(), |view, error| {
                view.child(div().text_color(cx.theme().danger).child(error))
            })
    }
}

impl Studio {
    pub(super) fn duplicate_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() >= MAX_TABS {
            self.notice = "Close a query before duplicating another.".into();
            cx.notify();
            return;
        }
        let original = &self.tabs[self.active];
        let source = original.editor.read(cx).value().to_string();
        let title = format!("{} (copy)", original.title);
        let connection_id = original.connection_id.clone();
        self.add_query(title, source, window, cx);
        // A copy is a new draft; result provenance and saved identity are not inherited.
        self.tabs[self.active].connection_id = connection_id;
        self.refresh_completions(cx);
    }

    pub(super) fn rename_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.tabs[self.active];
        let id = tab.id;
        let title = tab.title.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        let form = cx.new(|_| RenameForm { input, error: None });
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let form_for_save = form.clone();
            let view = view.clone();
            dialogs::frame(dialog, "Query workspace", "Name this query", "A useful name makes saved queries easier to find.", 440., cx).child(form.clone())
                .footer(h_flex().justify_end().gap_2()
                    .child(Button::new("cancel-rename").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("confirm-rename").primary().label("Rename").on_click(move |_, window, cx| {
                        let title = form_for_save.read(cx).input.read(cx).value().trim().to_string();
                        if title.is_empty() || title.chars().count() > 128 || title.len() > 512 || title.chars().any(char::is_control) {
                            form_for_save.update(cx, |form, cx| { form.error = Some("Enter a title of 1 to 128 characters without control characters.".into()); cx.notify(); });
                            return;
                        }
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| {
                                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                                    tab.title = title.clone();
                                    if let Some(saved_id) = tab.saved_id.clone() {
                                        for tab in &mut this.tabs { if tab.saved_id.as_ref() == Some(&saved_id) { tab.title = title.clone(); } }
                                        if let Some(query) = this.workspace.queries.iter_mut().find(|query| query.id == saved_id) { query.title = title; }
                                        this.persist();
                                    }
                                    cx.notify();
                                }
                            });
                        }
                        window.close_dialog(cx);
                    })))
        });
    }
}
