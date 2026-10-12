//! Native Kit theme roles translated from the installed DBX neutral theme.
//!
//! Keep component colors in the global theme so inputs, menus, tables and the
//! Base resize/scroll layer share the same appearance. Syntax and status colors
//! remain owned by the Kit's matching light/dark theme.

use gpui_kit::component::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, Window, px, rgb, rgba};

pub fn apply(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    // Loading the matching Kit theme first preserves its editor syntax palette.
    Theme::change(mode, window, cx);
    Theme::update(cx, |theme| {
        theme.colors = palette(mode);
        theme.radius = px(4.);
        theme.radius_lg = px(6.);
        theme.focus_ring = true;
        theme.shadow = false;
    });
}

fn palette(mode: ThemeMode) -> ThemeColor {
    let dark = mode.is_dark();
    let mut colors = if dark {
        *ThemeColor::dark()
    } else {
        *ThemeColor::light()
    };
    let color = |light, dark_value| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
    colors.background = color(0xffffff, 0x131416);
    colors.foreground = color(0x0a0a0a, 0xd7d7db);
    colors.primary = color(0x171717, 0xd0d0d6);
    colors.primary_foreground = color(0xfafafa, 0x131416);
    colors.primary_hover = color(0x303030, 0xbdbdc6);
    colors.primary_active = color(0x404040, 0xaaaab5);
    colors.secondary = color(0xf5f5f5, 0x2a2a2d);
    colors.secondary_foreground = colors.foreground;
    colors.secondary_hover = color(0xebebeb, 0x363639);
    colors.secondary_active = color(0xe5e5e5, 0x404044);
    colors.muted = colors.secondary;
    colors.muted_foreground = color(0x737373, 0x97989d);
    colors.accent = color(0xf5f5f5, 0x2e2f33);
    colors.accent_foreground = colors.foreground;
    colors.border = if dark {
        rgba(0x6e6e7247).into()
    } else {
        rgb(0xe5e5e5).into()
    };
    colors.input = if dark {
        rgba(0x6e6e7257).into()
    } else {
        colors.border
    };
    // DBX's light ring (#a1a1a1) is below 3:1 against white. Use its muted
    // neutral for visible keyboard focus while preserving the observed hue.
    colors.ring = color(0x737373, 0x85868b);
    colors.drag_border = colors.ring;
    colors.caret = colors.foreground;
    colors.selection = color(0xe5e5e5, 0x3b3c43);
    colors.popover = color(0xffffff, 0x1e1e20);
    colors.popover_foreground = colors.foreground;
    colors.group_box = color(0xffffff, 0x1b1b1e);
    colors.group_box_foreground = colors.foreground;
    colors.accordion = colors.background;
    colors.sidebar = color(0xfafafa, 0x19191c);
    colors.sidebar_foreground = colors.foreground;
    colors.sidebar_border = colors.border;
    colors.sidebar_accent = colors.accent;
    colors.sidebar_accent_foreground = colors.foreground;
    colors.sidebar_primary = colors.primary;
    colors.sidebar_primary_foreground = colors.primary_foreground;
    colors.button = colors.background;
    colors.button_foreground = colors.foreground;
    colors.button_hover = colors.accent;
    colors.button_active = colors.secondary_active;
    colors.button_primary = colors.primary;
    colors.button_primary_foreground = colors.primary_foreground;
    colors.button_primary_hover = colors.primary_hover;
    colors.button_primary_active = colors.primary_active;
    colors.button_secondary = colors.secondary;
    colors.button_secondary_foreground = colors.secondary_foreground;
    colors.button_secondary_hover = colors.secondary_hover;
    colors.button_secondary_active = colors.secondary_active;
    colors.list = colors.background;
    colors.list_head = colors.muted;
    colors.list_even = colors.sidebar;
    colors.list_hover = colors.accent;
    colors.list_active = colors.selection;
    colors.list_active_border = colors.ring;
    colors.tab = colors.background;
    colors.tab_foreground = colors.muted_foreground;
    colors.tab_active = colors.background;
    colors.tab_active_foreground = colors.foreground;
    colors.tab_bar = colors.background;
    colors.tab_bar_segmented = colors.muted;
    colors.table = colors.background;
    colors.table_head = colors.muted;
    colors.table_head_foreground = colors.foreground;
    colors.table_foot = colors.sidebar;
    colors.table_foot_foreground = colors.foreground;
    colors.table_even = color(0xfcfcfc, 0x171719);
    colors.table_hover = colors.accent;
    colors.table_active = colors.selection;
    colors.table_active_border = colors.ring;
    colors.table_row_border = colors.border;
    colors.title_bar = colors.sidebar;
    colors.title_bar_border = colors.border;
    colors.status_bar = colors.sidebar;
    colors.status_bar_border = colors.border;
    colors.description_list_label = colors.muted;
    colors.description_list_label_foreground = colors.foreground;
    colors.scrollbar_thumb = colors.muted_foreground.alpha(0.5);
    colors.scrollbar_thumb_hover = colors.muted_foreground;
    colors.skeleton = colors.muted;
    colors.switch = colors.muted;
    colors.switch_thumb = colors.primary_foreground;
    colors.slider_bar = colors.primary;
    colors.slider_thumb = colors.primary;
    colors.progress_bar = colors.primary;
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let luminance = |color: Hsla| {
            let color = color.to_rgb();
            let linear = |channel: f32| {
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        };
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn both_appearances_preserve_readable_text_and_visible_focus() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let colors = palette(mode);
            for (foreground, background) in [
                (colors.foreground, colors.background),
                (colors.muted_foreground, colors.background),
                (colors.muted_foreground, colors.sidebar),
                (colors.primary_foreground, colors.primary),
                (colors.foreground, colors.selection),
            ] {
                assert!(
                    contrast(foreground, background) >= 4.5,
                    "{mode:?}: {foreground:?} on {background:?}"
                );
            }
            assert!(contrast(colors.ring, colors.background) >= 3.);
        }
    }

    #[test]
    fn syntax_status_and_chart_roles_retain_the_kit_palette() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let base = if mode.is_dark() {
                *ThemeColor::dark()
            } else {
                *ThemeColor::light()
            };
            let colors = palette(mode);
            assert_eq!(colors.danger, base.danger);
            assert_eq!(colors.success, base.success);
            assert_eq!(colors.chart_1, base.chart_1);
            assert_ne!(colors.danger, colors.foreground);
        }
    }
}
