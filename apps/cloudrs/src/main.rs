//! cloudrs: the desktop app and composition root.

// A release build is a GUI app on Windows: no console window next to it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod comments;
mod i18n;
mod intent;
mod mini_player;
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

/// Logs to stderr, or to the file in `CLOUDRS_LOG_FILE`: the Windows release has no console.
fn init_logging() {
    let file = std::env::var_os("CLOUDRS_LOG_FILE").map(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    });
    // A log file also turns on the Jam's details; `RUST_LOG` always wins.
    let default = match file {
        Some(Ok(_)) => "cloudrs=info,sc_core::core::jam=debug,warn",
        _ => "cloudrs=info,warn",
    };
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| default.into());
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match file {
        Some(Ok(file)) => builder
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init(),
        Some(Err(error)) => {
            builder.init();
            tracing::warn!(%error, "could not open CLOUDRS_LOG_FILE; logging to stderr");
        }
        None => builder.init(),
    }
}

fn main() {
    init_logging();

    // The sign-in window runs as this same executable (ADR 0010).
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some(sc_platform::sign_in::ARG) {
        let title = args.next().unwrap_or_default();
        sc_platform::sign_in::run_window(&title);
    }
    let oauth_token = sc_platform::keychain::load_token();
    // Read before the window opens, so the first frame has the right theme (ADR 0017).
    let data_dir = app_dir(dirs::data_dir(), "data");
    // With nothing saved, the first run follows the system language (ADR 0025); an
    // unreadable database keeps the defaults rather than guessing.
    let settings = match sc_core::read_saved_settings(&data_dir) {
        Ok(Some(saved)) => saved,
        Ok(None) => sc_core::Settings {
            language: i18n::system_language(),
            ..sc_core::Settings::default()
        },
        Err(error) => {
            tracing::warn!(%error, "could not read the settings; using the defaults");
            sc_core::Settings::default()
        }
    };

    application()
        .with_assets(cloudrs_ui::assets::Assets)
        .run(move |cx: &mut App| {
            cloudrs_ui::fonts::register(cx);
            appearance::apply(&settings, cx);
            cloudrs_ui::search_field::bind_keys(cx);
            shell::bind_keys(cx);

            let config = CoreConfig {
                cache_dir: app_dir(dirs::cache_dir(), "cache"),
                data_dir,
                oauth_token,
                jam_network: sc_core::JamNetwork::Internet,
                settings: settings.clone(),
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
