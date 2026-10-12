mod credentials;
mod data_ops;
mod database;
mod sql_completion;
mod sql_tools;
mod studio;
mod studio_theme;
mod table_browse;
mod updater;
mod workspace;

#[cfg(test)]
extern crate gpui_kit as gpui;

use gpui_kit::{AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use studio::Studio;

fn main() {
    match updater::install::run_helper_if_requested() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("Relix update failed: {error:#}");
            std::process::exit(1);
        }
    }
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("Relix {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            studio::bind_keys(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(820.)),
                    cx,
                ))),
                window_min_size: Some(size(px(900.), px(620.))),
                titlebar: Some(gpui_kit::TitlebarOptions {
                    title: Some("Relix — Database Studio".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Studio::new(window, cx))
            }) {
                eprintln!("Unable to open Relix: {error}");
                cx.quit();
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        });
}
