//! Plays a stream URL (HLS playlist or plain file) on the default device.
//!
//! ```sh
//! cargo run -p sc-audio --example play -- https://example.com/stream.m3u8
//! # A SoundCloud track, through sc-api:
//! cargo run -p sc-audio --example play -- "$(cargo run -q -p sc-api --example stream -- "lights out")"
//! ```

use std::io::Write;

use sc_audio::{Command, Event, PlaybackState, Player, Source};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("usage: play <stream url>");
        std::process::exit(2);
    };
    let player = Player::spawn()?;
    player.send(Command::Load(Source::from_url(url)))?;
    for event in player.events().iter() {
        match event {
            Event::Position(position) => {
                let secs = position.as_secs();
                print!("\r▶ {:02}:{:02}", secs / 60, secs % 60);
                std::io::stdout().flush()?;
            }
            Event::State(PlaybackState::Ended) => {
                println!("\nended");
                break;
            }
            Event::State(state) => eprintln!("\n{state:?}"),
            Event::Error(error) => {
                eprintln!("\nerror: {error}");
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
