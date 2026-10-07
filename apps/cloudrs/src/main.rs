//! cloudrs: the desktop app and composition root.

// A release build is a GUI app on Windows: no console window next to it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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

use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    point, px, size,
};
use gpui_platform::application;
use sc_core::CoreConfig;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cloudrs=info,warn".into()),
        )
        .init();

    // The sign-in window runs as this same executable (ADR 0010).
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some(sc_platform::sign_in::ARG) {
        let title = args.next().unwrap_or_default();
        sc_platform::sign_in::run_window(&title);
    }
    let oauth_token = sc_platform::keychain::load_token();

    application()
        .with_assets(cloudrs_ui::assets::Assets)
        .run(move |cx: &mut App| {
            cloudrs_ui::fonts::register(cx);
            cx.set_global(cloudrs_ui::ThemeMode::default());
            cloudrs_ui::search_field::bind_keys(cx);
            shell::bind_keys(cx);

            let config = CoreConfig {
                cache_dir: app_dir(dirs::cache_dir(), "cache"),
                data_dir: app_dir(dirs::data_dir(), "data"),
                oauth_token,
                jam_network: sc_core::JamNetwork::Internet,
            };
            let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(i18n::app::window_title().into()),
                        // The app draws its own title bar (shell::title_bar).
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(16.0), px(20.0))),
                    }),
                    window_decorations: Some(WindowDecorations::Client),
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
