//! Prints the stream URL of a track, given its soundcloud.com URL or a search
//! query (the first result is used). Pipe it into the audio engine:
//!
//! ```sh
//! cargo run -p sc-audio --example play -- "$(cargo run -q -p sc-api --example stream -- "lights out")"
//! ```

use sc_api::models::Resource;
use sc_api::{ClientConfig, ScClient, SoundCloudApi};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if input.is_empty() {
        eprintln!("usage: stream <soundcloud url | search query>");
        std::process::exit(2);
    }
    let sc = ScClient::new(ClientConfig::default())?;
    let track = if input.starts_with("https://") {
        match sc.resolve(&input).await? {
            Resource::Track(track) => *track,
            _ => return Err("that URL is not a track".into()),
        }
    } else {
        let page = sc.search_tracks(&input, 1).await?;
        page.collection.into_iter().next().ok_or("no results")?
    };
    let stream = sc.stream_url(&track).await?;
    eprintln!(
        "{} ({:?}, {})",
        track.title, stream.protocol, stream.mime_type
    );
    println!("{}", stream.url);
    Ok(())
}
