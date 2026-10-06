//! cloudrs: the desktop app and composition root.

mod i18n;
mod preview;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cloudrs=info,warn".into()),
        )
        .init();

    application().run(|cx: &mut App| {
        cloudrs_ui::fonts::register(cx);
        cx.set_global(cloudrs_ui::ThemeMode::default());

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
            |_, cx| cx.new(|_| preview::Preview::new()),
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
