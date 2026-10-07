//! The core's disk state in SQLite: the session (queue, position, volume) and
//! the listening history. Plain functions on a connection; the core runs them
//! with `spawn_blocking` so the actor never waits on disk.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::{Repeat, TrackId, TrackSummary};

/// Bumped with every schema change; `migrate` upgrades older files.
const SCHEMA_VERSION: i32 = 1;

/// A queued track with what is needed to show it without the network.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionTrack {
    pub track: TrackSummary,
    pub artwork_url: Option<String>,
}

/// What survives a restart.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub tracks: Vec<SessionTrack>,
    pub current: Option<usize>,
    pub position: Duration,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE session (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 current INTEGER,
                 position_ms INTEGER NOT NULL,
                 volume REAL NOT NULL,
                 shuffle INTEGER NOT NULL,
                 repeat INTEGER NOT NULL
             );
             CREATE TABLE queue_items (
                 position INTEGER PRIMARY KEY,
                 track_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 duration_ms INTEGER NOT NULL,
                 preview_only INTEGER NOT NULL,
                 artwork_url TEXT
             );
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 track_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 played_at INTEGER NOT NULL
             );
             CREATE INDEX history_played_at ON history (played_at);",
        )?;
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
}

fn repeat_to_int(repeat: Repeat) -> i64 {
    match repeat {
        Repeat::Off => 0,
        Repeat::One => 1,
        Repeat::All => 2,
    }
}

fn repeat_from_int(value: i64) -> Repeat {
    match value {
        1 => Repeat::One,
        2 => Repeat::All,
        _ => Repeat::Off,
    }
}

/// Replaces the saved session in one transaction.
pub fn save_session(conn: &mut Connection, session: &Session) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO session (id, current, position_ms, volume, shuffle, repeat)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            session.current.map(|i| i as i64),
            session.position.as_millis() as i64,
            f64::from(session.volume),
            session.shuffle,
            repeat_to_int(session.repeat),
        ],
    )?;
    tx.execute("DELETE FROM queue_items", [])?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO queue_items
                 (position, track_id, title, artist, duration_ms, preview_only, artwork_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for (position, item) in session.tracks.iter().enumerate() {
            let track = &item.track;
            insert.execute(params![
                position as i64,
                track.id.0 as i64,
                track.title,
                track.artist,
                track.duration.as_millis() as i64,
                track.preview_only,
                item.artwork_url,
            ])?;
        }
    }
    tx.commit()
}

pub fn load_session(conn: &Connection) -> rusqlite::Result<Option<Session>> {
    let header = conn
        .query_row(
            "SELECT current, position_ms, volume, shuffle, repeat FROM session WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((current, position_ms, volume, shuffle, repeat)) = header else {
        return Ok(None);
    };
    let mut query = conn.prepare(
        "SELECT track_id, title, artist, duration_ms, preview_only, artwork_url
         FROM queue_items ORDER BY position",
    )?;
    let tracks = query
        .query_map([], |row| {
            Ok(SessionTrack {
                track: TrackSummary {
                    id: TrackId(row.get::<_, i64>(0)? as u64),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    duration: Duration::from_millis(row.get::<_, i64>(3)?.max(0) as u64),
                    preview_only: row.get(4)?,
                },
                artwork_url: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(Session {
        tracks,
        current: current.map(|i| i as usize),
        position: Duration::from_millis(position_ms.max(0) as u64),
        volume: volume as f32,
        shuffle,
        repeat: repeat_from_int(repeat),
    }))
}

/// Adds a played track to the history. `played_at` is Unix seconds.
pub fn record_play(
    conn: &Connection,
    track: &TrackSummary,
    played_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO history (track_id, title, artist, played_at) VALUES (?1, ?2, ?3, ?4)",
        params![track.id.0 as i64, track.title, track.artist, played_at],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn item(id: u64, art: Option<&str>) -> SessionTrack {
        SessionTrack {
            track: TrackSummary {
                id: TrackId(id),
                title: format!("Title {id}"),
                artist: "Artist".into(),
                duration: Duration::from_millis(215_500),
                preview_only: id.is_multiple_of(2),
            },
            artwork_url: art.map(str::to_owned),
        }
    }

    fn session(ids: &[u64]) -> Session {
        Session {
            tracks: ids
                .iter()
                .map(|id| item(*id, Some("https://a/x.jpg")))
                .collect(),
            current: Some(1),
            position: Duration::from_millis(42_250),
            volume: 0.5,
            shuffle: true,
            repeat: Repeat::All,
        }
    }

    #[test]
    fn an_empty_database_has_no_session() {
        assert_eq!(load_session(&memory()).unwrap(), None);
    }

    #[test]
    fn a_session_round_trips() {
        let mut conn = memory();
        let saved = session(&[1, 2, 3]);
        save_session(&mut conn, &saved).unwrap();
        assert_eq!(load_session(&conn).unwrap(), Some(saved));
    }

    #[test]
    fn saving_replaces_the_previous_session() {
        let mut conn = memory();
        save_session(&mut conn, &session(&[1, 2, 3])).unwrap();
        let mut second = session(&[9]);
        second.current = None;
        second.tracks[0].artwork_url = None;
        save_session(&mut conn, &second).unwrap();
        assert_eq!(load_session(&conn).unwrap(), Some(second));
    }

    #[test]
    fn history_keeps_every_play() {
        let conn = memory();
        let track = item(5, None).track;
        record_play(&conn, &track, 1_000).unwrap();
        record_play(&conn, &track, 2_000).unwrap();
        let rows: Vec<(i64, i64)> = conn
            .prepare("SELECT track_id, played_at FROM history ORDER BY played_at")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows, [(5, 1_000), (5, 2_000)]);
    }

    #[test]
    fn opening_twice_keeps_data_and_the_schema_version() {
        let dir = std::env::temp_dir().join(format!("cloudrs-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let _ = std::fs::remove_file(&path);

        let mut conn = open(&path).unwrap();
        save_session(&mut conn, &session(&[1])).unwrap();
        drop(conn);

        let conn = open(&path).unwrap();
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert!(load_session(&conn).unwrap().is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
