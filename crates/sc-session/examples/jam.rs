//! Try a Jam connection between two machines, without the app:
//!
//! ```sh
//! cargo run -p sc-session --example jam -- host
//! cargo run -p sc-session --example jam -- join "cloudrs:jam/..."
//! ```
//!
//! The host prints its link and each guest that joins; a guest prints the
//! clock offset to the host and its round trip every few seconds.

use std::time::Duration;

use sc_session::{Network, Profile, Session, SessionCommand, SessionEvent, ToGuest};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let session = match args.first().map(String::as_str) {
        Some("host") => Session::host(Network::Internet),
        Some("join") if args.len() > 1 => Session::join(
            &args[1],
            Profile {
                name: whoami(),
                ..Profile::default()
            },
        ),
        _ => {
            eprintln!("usage: jam host | jam join <link>");
            std::process::exit(2);
        }
    };
    let started = std::time::Instant::now();
    while let Ok(event) = session.events().recv() {
        let at = started.elapsed().as_secs_f32();
        match event {
            SessionEvent::Started { link } => println!("[{at:6.1}s] share this link:\n{link}"),
            SessionEvent::PeerJoined { peer, profile } => {
                let name = &profile.name;
                println!("[{at:6.1}s] {name} joined as {peer:?}");
                session.send(SessionCommand::SendTo(
                    peer,
                    ToGuest::Peers { peers: Vec::new() },
                ));
            }
            SessionEvent::ClockOffset { offset_ns, rtt } => println!(
                "[{at:6.1}s] clock offset {:.1} ms, round trip {:.1} ms",
                offset_ns as f64 / 1e6,
                rtt.as_secs_f64() * 1e3
            ),
            SessionEvent::Ended(ended) => {
                println!("[{at:6.1}s] ended: {ended:?}");
                break;
            }
            other => println!("[{at:6.1}s] {other:?}"),
        }
    }
    std::thread::sleep(Duration::from_millis(100));
}

fn whoami() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "guest".into())
}
