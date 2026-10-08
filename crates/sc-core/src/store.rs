//! The core's disk state in SQLite: the session (queue, position, volume) and
//! the listening history. Plain functions on a connection; the core runs them
//! with `spawn_blocking` so the actor never waits on disk.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

use crate::settings::{Language, Settings, ThemeChoice};
use crate::types::{Repeat, TrackId, TrackSummary};

/// Name of the database file inside the data folder.
pub const FILE_NAME: &str = "cloudrs.db";
/// Bumped with every schema change; `migrate` upgrades older files.
const SCHEMA_VERSION: i32 = 4;
/// How long a query waits when another instance has the file locked.
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

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

/// Why the database could not be opened.
#[derive(Debug)]
pub enum OpenError {
    /// The file is not a database (or is damaged): it can be set aside.
    Corrupt,
    /// Written by a newer build. It is left untouched and not used, so an older
    /// build never downgrades it.
    Newer(i32),
    Io(std::io::Error),
    Other(rusqlite::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt => write!(f, "the file is not a valid database"),
            Self::Newer(version) => write!(f, "the schema (version {version}) is newer"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Other(error) => write!(f, "{error}"),
        }
    }
}

fn classify(error: rusqlite::Error) -> OpenError {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    match error.sqlite_error_code() {
        Some(NotADatabase | DatabaseCorrupt) => OpenError::Corrupt,
        _ => OpenError::Other(error),
    }
}

/// Opens (and migrates) the database at `path`.
pub fn open(path: &Path) -> Result<Connection, OpenError> {
    let conn = Connection::open(path).map_err(classify)?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(classify)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Like [`open`], but a damaged file is renamed to `<name>.corrupt-<unix time>`
/// and a new one is created. The flag says the data was reset.
pub fn open_or_reset(path: &Path) -> Result<(Connection, bool), OpenError> {
    match open(path) {
        Ok(conn) => Ok((conn, false)),
        Err(OpenError::Corrupt) => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let name = path.file_name().map_or_else(
                || FILE_NAME.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
            std::fs::rename(path, path.with_file_name(format!("{name}.corrupt-{stamp}")))
                .map_err(OpenError::Io)?;
            Ok((open(path)?, true))
        }
        Err(error) => Err(error),
    }
}

/// Upgrades the schema in one transaction, `user_version` included, so a
/// failure leaves the old version intact.
fn migrate(conn: &Connection) -> Result<(), OpenError> {
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(classify)?;
    if version > SCHEMA_VERSION {
        return Err(OpenError::Newer(version));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = conn.unchecked_transaction().map_err(classify)?;
    if version < 1 {
        tx.execute_batch(
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
        )
        .map_err(classify)?;
    }
    if version < 2 {
        // The history screen lists rows like any other track list. Rows
        // written before this version keep a zero duration and no cover.
        tx.execute_batch(
            "ALTER TABLE history ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE history ADD COLUMN preview_only INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE history ADD COLUMN artwork_url TEXT;",
        )
        .map_err(classify)?;
    }
    if version < 3 {
        // One row, like `session`: the theme as a code, the language as its tag.
        tx.execute_batch(
            "CREATE TABLE settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 theme INTEGER NOT NULL,
                 language TEXT NOT NULL,
                 discord INTEGER NOT NULL
             );",
        )
        .map_err(classify)?;
    }
    if version < 4 {
        // The chosen output device's id; NULL follows the system default.
        tx.execute_batch("ALTER TABLE settings ADD COLUMN output_device TEXT;")
            .map_err(classify)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(classify)?;
    tx.commit().map_err(classify)
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

fn theme_to_int(theme: ThemeChoice) -> i64 {
    match theme {
        ThemeChoice::System => 0,
        ThemeChoice::Dark => 1,
        ThemeChoice::Light => 2,
    }
}

fn theme_from_int(value: i64) -> ThemeChoice {
    match value {
        1 => ThemeChoice::Dark,
        2 => ThemeChoice::Light,
        _ => ThemeChoice::System,
    }
}

/// Replaces the saved settings.
pub fn save_settings(conn: &Connection, settings: &Settings) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (id, theme, language, discord, output_device)
         VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            theme_to_int(settings.theme),
            settings.language.tag(),
            settings.discord,
            settings.output_device
        ],
    )?;
    Ok(())
}

/// The saved settings, if any were ever saved.
pub fn load_settings(conn: &Connection) -> rusqlite::Result<Option<Settings>> {
    conn.query_row(
        "SELECT theme, language, discord, output_device FROM settings WHERE id = 1",
        [],
        |row| {
            Ok(Settings {
                theme: theme_from_int(row.get(0)?),
                language: Language::from_tag(&row.get::<_, String>(1)?),
                discord: row.get(2)?,
                output_device: row.get(3)?,
            })
        },
    )
    .optional()
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
                    artist_id: None,
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
    artwork_url: Option<&str>,
    played_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO history
             (track_id, title, artist, duration_ms, preview_only, artwork_url, played_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            track.id.0 as i64,
            track.title,
            track.artist,
            track.duration.as_millis() as i64,
            track.preview_only,
            artwork_url,
            played_at
        ],
    )?;
    Ok(())
}

/// The most recently played tracks, newest first, each track once.
pub fn recent_history(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<SessionTrack>> {
    let mut query = conn.prepare(
        "SELECT track_id, title, artist, duration_ms, preview_only, artwork_url
         FROM history
         WHERE id IN (SELECT MAX(id) FROM history GROUP BY track_id)
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    query
        .query_map([limit], |row| {
            Ok(SessionTrack {
                track: TrackSummary {
                    id: TrackId(row.get::<_, i64>(0)? as u64),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    artist_id: None,
                    duration: Duration::from_millis(row.get::<_, i64>(3)?.max(0) as u64),
                    preview_only: row.get(4)?,
                },
                artwork_url: row.get(5)?,
            })
        })?
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cloudrs-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(id: u64, art: Option<&str>) -> SessionTrack {
        SessionTrack {
            track: TrackSummary {
                id: TrackId(id),
                title: format!("Title {id}"),
                artist: "Artist".into(),
                artist_id: None,
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
    fn settings_round_trip() {
        let conn = memory();
        let saved = Settings {
            theme: ThemeChoice::Light,
            language: Language::English,
            discord: false,
            output_device: Some("wasapi:x".into()),
        };
        save_settings(&conn, &saved).unwrap();
        assert_eq!(load_settings(&conn).unwrap(), Some(saved.clone()));
        let dark = Settings {
            theme: ThemeChoice::Dark,
            ..saved
        };
        save_settings(&conn, &dark).unwrap();
        assert_eq!(load_settings(&conn).unwrap(), Some(dark));
    }

    #[test]
    fn an_empty_database_has_no_settings() {
        assert_eq!(load_settings(&memory()).unwrap(), None);
    }

    #[test]
    fn unknown_theme_codes_read_as_system() {
        let conn = memory();
        conn.execute(
            "INSERT INTO settings (id, theme, language, discord) VALUES (1, 77, 'xx', 1)",
            [],
        )
        .unwrap();
        let loaded = load_settings(&conn).unwrap().unwrap();
        assert_eq!(loaded.theme, ThemeChoice::System);
        assert_eq!(loaded.language, Language::English);
    }

    #[test]
    fn a_version_two_database_gains_the_settings_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id INTEGER);
             CREATE TABLE queue_items (position INTEGER);
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 track_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 played_at INTEGER NOT NULL,
                 duration_ms INTEGER NOT NULL DEFAULT 0,
                 preview_only INTEGER NOT NULL DEFAULT 0,
                 artwork_url TEXT
             );
             PRAGMA user_version = 2;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(load_settings(&conn).unwrap(), None);
        let saved = Settings {
            theme: ThemeChoice::Dark,
            ..Settings::default()
        };
        save_settings(&conn, &saved).unwrap();
        assert_eq!(load_settings(&conn).unwrap(), Some(saved));
    }

    #[test]
    fn a_version_three_database_gains_the_output_device() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id INTEGER);
             CREATE TABLE queue_items (position INTEGER);
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 track_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 played_at INTEGER NOT NULL,
                 duration_ms INTEGER NOT NULL DEFAULT 0,
                 preview_only INTEGER NOT NULL DEFAULT 0,
                 artwork_url TEXT
             );
             CREATE TABLE settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 theme INTEGER NOT NULL,
                 language TEXT NOT NULL,
                 discord INTEGER NOT NULL
             );
             INSERT INTO settings (id, theme, language, discord) VALUES (1, 1, 'en', 0);
             PRAGMA user_version = 3;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let loaded = load_settings(&conn).unwrap().unwrap();
        assert_eq!(loaded.theme, ThemeChoice::Dark);
        assert_eq!(loaded.output_device, None);
        let saved = Settings {
            output_device: Some("wasapi:x".into()),
            ..loaded
        };
        save_settings(&conn, &saved).unwrap();
        assert_eq!(load_settings(&conn).unwrap(), Some(saved));
    }

    #[test]
    fn history_keeps_every_play() {
        let conn = memory();
        let track = item(5, None).track;
        record_play(&conn, &track, None, 1_000).unwrap();
        record_play(&conn, &track, None, 2_000).unwrap();
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
    fn recent_history_lists_each_track_once_newest_first() {
        let conn = memory();
        let (a, b) = (item(1, None).track, item(2, None).track);
        record_play(&conn, &a, Some("https://a/1.jpg"), 1_000).unwrap();
        record_play(&conn, &b, None, 2_000).unwrap();
        record_play(&conn, &a, Some("https://a/1.jpg"), 3_000).unwrap();

        let rows = recent_history(&conn, 10).unwrap();
        let ids: Vec<u64> = rows.iter().map(|row| row.track.id.0).collect();
        assert_eq!(ids, [1, 2]);
        assert_eq!(rows[0].track, a);
        assert_eq!(rows[0].artwork_url.as_deref(), Some("https://a/1.jpg"));
        assert_eq!(recent_history(&conn, 1).unwrap().len(), 1);
    }

    #[test]
    fn a_version_one_database_gains_the_history_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id INTEGER);
             CREATE TABLE queue_items (position INTEGER);
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 track_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 played_at INTEGER NOT NULL
             );
             INSERT INTO history (track_id, title, artist, played_at) VALUES (7, 'Old', 'A', 1);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let rows = recent_history(&conn, 10).unwrap();
        assert_eq!(rows[0].track.title, "Old");
        assert_eq!(rows[0].track.duration, Duration::ZERO);
    }

    #[test]
    fn opening_twice_keeps_data_and_the_schema_version() {
        let dir = temp_dir("twice");
        let path = dir.join("test.db");

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

    #[test]
    fn a_damaged_file_is_set_aside_and_replaced() {
        let dir = temp_dir("corrupt");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            b"this is not a sqlite database, just text".repeat(50),
        )
        .unwrap();

        let (conn, reset) = open_or_reset(&path).unwrap();
        assert!(reset);
        assert_eq!(load_session(&conn).unwrap(), None);
        drop(conn);

        let aside: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("cloudrs.db.corrupt-"))
            .collect();
        assert_eq!(aside.len(), 1);

        // The next start opens the new file normally.
        assert!(!open_or_reset(&path).unwrap().1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_database_is_left_alone() {
        let dir = temp_dir("newer");
        let path = dir.join(FILE_NAME);
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);

        assert!(matches!(open_or_reset(&path), Err(OpenError::Newer(99))));
        let conn = Connection::open(&path).unwrap();
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 99, "not downgraded");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_migration_leaves_the_version_unchanged() {
        let conn = Connection::open_in_memory().unwrap();
        // A table in the way makes the schema creation fail halfway.
        conn.execute_batch("CREATE TABLE history (x INTEGER);")
            .unwrap();
        assert!(migrate(&conn).is_err());
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0, "the first tables were rolled back");
    }
}
