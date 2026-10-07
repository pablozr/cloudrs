//! cloudrs: the desktop app and composition root.

mod i18n;
mod intent;
mod models;
mod nav;
mod player_bar;
mod screens;
mod seam;
mod shell;
mod state;
mod tint;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;
use sc_core::CoreConfig;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cloudrs=info,warn".into()),
        )
        .init();

    application()
        .with_assets(cloudrs_ui::assets::Assets)
        .run(|cx: &mut App| {
            cloudrs_ui::fonts::register(cx);
            cx.set_global(cloudrs_ui::ThemeMode::default());
            cloudrs_ui::search_field::bind_keys(cx);
            shell::bind_keys(cx);

            let config = CoreConfig {
                cache_dir: app_dir(dirs::cache_dir(), "cache"),
                data_dir: app_dir(dirs::data_dir(), "data"),
            };
            let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(i18n::app::window_title().into()),
                        ..Default::default()
                    }),
                    app_id: Some("dev.cloudrs.cloudrs".into()),
                    window_min_size: Some(size(px(860.0), px(560.0))),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| shell::Shell::new(config, window, cx)),
            );
            if let Err(error) = opened {
                tracing::error!(%error, "failed to open the main window");
                cx.quit();
                return;
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}

/// `<OS dir>/cloudrs`, or the temp dir when the OS has none.
fn app_dir(base: Option<std::path::PathBuf>, what: &str) -> std::path::PathBuf {
    let base = base.unwrap_or_else(|| {
        let fallback = std::env::temp_dir();
        tracing::warn!(
            ?fallback,
            "no OS {what} directory; using the temp directory"
        );
        fallback
    });
    base.join("cloudrs")
}
