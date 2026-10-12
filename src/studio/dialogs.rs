//! Shared native dialog presentation; Kit retains focus and dismissal ownership.

use gpui_kit::component::{ActiveTheme, dialog::Dialog, v_flex};
use gpui_kit::{App, FontWeight, div, prelude::*, px};

pub(super) fn frame(
    dialog: Dialog,
    section: &str,
    title: impl Into<gpui_kit::SharedString>,
    description: &str,
    width: f32,
    cx: &App,
) -> Dialog {
    dialog
        .width(px(width))
        .margin_top(px(48.))
        .rounded(px(8.))
        .shadow_sm()
        .p_5()
        .title(
            v_flex()
                .gap_2()
                .pr_6()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(section.to_owned()),
                )
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(cx.theme().muted_foreground)
                        .child(description.to_owned()),
                ),
        )
}
