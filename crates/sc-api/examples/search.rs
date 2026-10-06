//! Searches SoundCloud from the terminal.
//!
//! ```sh
//! cargo run -p sc-api --example search -- "charlotte de witte"
//! ```

use sc_api::{ClientConfig, ScClient, SoundCloudApi};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if query.is_empty() {
        eprintln!("usage: search <query>");
        std::process::exit(2);
    }
    let sc = ScClient::new(ClientConfig::default())?;
    let page = sc.search_tracks(&query, 10).await?;
    println!(
        "{} results (client_id {})",
        page.total_results.unwrap_or(page.collection.len() as u64),
        sc.client_id().await?
    );
    for track in &page.collection {
        let user = track.user.as_ref().map_or("?", |u| u.username.as_str());
        let secs = track.duration / 1000;
        let formats: Vec<_> = track
            .media
            .transcodings
            .iter()
            .map(|t| format!("{}:{}", t.format.protocol, t.preset))
            .collect();
        println!(
            "{:>11}  {:>2}:{:02}  {} · {}\n             {}",
            track.id,
            secs / 60,
            secs % 60,
            track.title,
            user,
            formats.join(" ")
        );
    }
    Ok(())
}
