//! Signing in on soundcloud.com in a small webview window (ADR 0010).
//!
//! The app starts itself again with [`ARG`]; that process runs
//! [`run_window`], which prints the `oauth_token` cookie on stdout once the
//! person has signed in, and exits. The webview is private, so nothing it
//! sees is kept on disk. Running it in its own process keeps a second event
//! loop out of the app.

use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

/// The argument that makes the app run the sign-in window instead.
pub const ARG: &str = "--sign-in";

const SIGN_IN_URL: &str = "https://soundcloud.com/signin";
const COOKIE_URL: &str = "https://soundcloud.com";
const COOKIE: &str = "oauth_token";
/// How often the cookie is checked between page loads.
const POLL: Duration = Duration::from_secs(1);

/// Starts the sign-in window as a child process and waits for it. Returns the
/// token, `None` when the person closed the window, or an error when the
/// window could not open. Blocks: call it off the UI thread.
pub fn sign_in_with_window(title: &str) -> std::io::Result<Option<String>> {
    let mut child = Command::new(std::env::current_exe()?)
        .args([ARG, title])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut line = String::new();
    if let Some(stdout) = child.stdout.take() {
        std::io::BufReader::new(stdout).read_line(&mut line)?;
    }
    if !child.wait()?.success() {
        return Err(std::io::Error::other("the sign-in window failed"));
    }
    let token = line.trim();
    Ok((!token.is_empty()).then(|| token.to_owned()))
}

/// The page finished loading: time to look for the cookie.
struct PageLoaded;

/// Runs the sign-in window on this thread (the process's main thread) until
/// the token appears or the window closes, then exits the process.
pub fn run_window(title: &str) -> ! {
    let event_loop = EventLoopBuilder::<PageLoaded>::with_user_event().build();
    let window = match WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(480.0, 720.0))
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(error) => fail(&error.to_string()),
    };
    let proxy = event_loop.create_proxy();
    let builder = WebViewBuilder::new()
        .with_url(SIGN_IN_URL)
        .with_incognito(true)
        .with_on_page_load_handler(move |_, _| {
            let _ = proxy.send_event(PageLoaded);
        });
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window);
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        match window.default_vbox() {
            Some(vbox) => builder.build_gtk(vbox),
            None => fail("no GTK container for the webview"),
        }
    };
    let webview = match webview {
        Ok(webview) => webview,
        Err(error) => fail(&error.to_string()),
    };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + POLL);
        let check = matches!(
            event,
            Event::UserEvent(PageLoaded)
                | Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. })
        );
        if check && let Some(token) = token(&webview) {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{token}");
            let _ = out.flush();
            std::process::exit(0);
        }
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            std::process::exit(0);
        }
    })
}

fn token(webview: &wry::WebView) -> Option<String> {
    let cookies = webview.cookies_for_url(COOKIE_URL).ok()?;
    cookies
        .iter()
        .find(|cookie| cookie.name() == COOKIE && !cookie.value().is_empty())
        .map(|cookie| cookie.value().to_owned())
}

fn fail(error: &str) -> ! {
    tracing::error!(%error, "the sign-in window could not open");
    std::process::exit(1);
}
