//! Fetch + decode against a local HTTP server serving `tests/assets`
//! (generated with ffmpeg; see `tests/assets/README.md`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

use sc_audio::{Decoder, Error, Source, open};

/// Serves files from `tests/assets` until the test process exits.
fn serve_assets() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let root = root.clone();
            thread::spawn(move || {
                let mut request = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                reader.read_line(&mut request).unwrap();
                // Skip the headers.
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    line.clear();
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap().trim_start_matches('/');
                match std::fs::read(root.join(path)) {
                    Ok(body) => {
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .unwrap();
                        stream.write_all(&body).unwrap();
                    }
                    Err(_) => {
                        stream
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .unwrap();
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

/// Decodes a whole source and returns (seconds, sample rate, channels).
fn decode_all(url: &str) -> (f64, u32, usize) {
    let opened = open(&Source::from_url(url)).unwrap();
    let mut decoder = Decoder::new(opened.reader, opened.extension.as_deref()).unwrap();
    let mut chunk = Vec::new();
    let mut samples = 0usize;
    while decoder.next_chunk(&mut chunk).unwrap() {
        samples += chunk.len();
    }
    let rate = decoder.sample_rate();
    let channels = decoder.channels();
    (
        samples as f64 / channels as f64 / f64::from(rate),
        rate,
        channels,
    )
}

fn assert_close(actual: f64, expected: f64) {
    // AAC adds up to two frames (~46 ms) of encoder priming.
    assert!(
        (actual - expected).abs() < 0.1,
        "decoded {actual:.3} s, expected {expected} s"
    );
}

#[test]
fn decodes_hls_with_fmp4_aac_segments() {
    let base = serve_assets();
    let (secs, rate, channels) = decode_all(&format!("{base}/fmp4.m3u8"));
    assert_eq!((rate, channels), (44_100, 2));
    assert_close(secs, 3.0);
}

#[test]
fn decodes_hls_with_adts_aac_segments() {
    let base = serve_assets();
    let (secs, rate, channels) = decode_all(&format!("{base}/adts.m3u8"));
    assert_eq!((rate, channels), (44_100, 2));
    assert_close(secs, 2.0);
}

#[test]
fn decodes_progressive_mp3() {
    let base = serve_assets();
    let (secs, rate, channels) = decode_all(&format!("{base}/tone.mp3"));
    assert_eq!((rate, channels), (48_000, 1));
    assert_close(secs, 1.0);
}

#[test]
fn reports_a_missing_playlist() {
    let base = serve_assets();
    let error = open(&Source::from_url(format!("{base}/missing.m3u8")))
        .err()
        .expect("missing playlist must fail");
    assert!(matches!(error, Error::Network(_)), "{error:?}");
}
