//! Finding the public `client_id` the soundcloud.com web app uses.
//!
//! The id lives in one of the JavaScript bundles linked from the home page and
//! changes from time to time, so it is extracted at runtime and refreshed when
//! SoundCloud starts refusing it.

use std::sync::LazyLock;

use regex::Regex;

static SCRIPT_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<script[^>]*\ssrc="([^"]+/assets/[^"]+\.js)""#).expect("valid regex")
});

static CLIENT_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"client_id\s*[:=]\s*"([A-Za-z0-9]{32})"|[?&]client_id=([A-Za-z0-9]{32})"#)
        .expect("valid regex")
});

/// The JavaScript bundles linked from a soundcloud.com page, in page order.
pub fn script_urls(html: &str) -> Vec<String> {
    SCRIPT_SRC
        .captures_iter(html)
        .map(|caps| caps[1].to_owned())
        .collect()
}

/// The first `client_id` found in a JavaScript bundle.
pub fn find_client_id(js: &str) -> Option<String> {
    CLIENT_ID.captures(js).and_then(|caps| {
        caps.get(1)
            .or_else(|| caps.get(2))
            .map(|id| id.as_str().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_asset_scripts_in_page_order() {
        let html = r#"<html><script crossorigin src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>
            <script src="https://example.com/other.js"></script>
            <script crossorigin src="https://a-v2.sndcdn.com/assets/49-def.js"></script></html>"#;
        assert_eq!(
            script_urls(html),
            [
                "https://a-v2.sndcdn.com/assets/0-abc.js",
                "https://a-v2.sndcdn.com/assets/49-def.js"
            ]
        );
    }

    #[test]
    fn finds_client_id_in_object_literal() {
        let js = r#"...,{client_id:"AbCdEfGhIjKlMnOpQrStUvWxYz012345",env:"production"}"#;
        assert_eq!(
            find_client_id(js).as_deref(),
            Some("AbCdEfGhIjKlMnOpQrStUvWxYz012345")
        );
    }

    #[test]
    fn finds_client_id_in_query_string() {
        let js = r#"fetch("https://api-v2.soundcloud.com/me?client_id=AbCdEfGhIjKlMnOpQrStUvWxYz012345&app_version=1")"#;
        assert_eq!(
            find_client_id(js).as_deref(),
            Some("AbCdEfGhIjKlMnOpQrStUvWxYz012345")
        );
    }

    #[test]
    fn ignores_ids_of_the_wrong_length() {
        assert_eq!(find_client_id(r#"client_id:"short""#), None);
    }
}
