//! The artwork cache: one file per artwork URL in the cache folder.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

/// Size requested from SoundCloud: enough for cards and the player bar.
pub const SIZE: &str = "t300x300";

/// Where the artwork for `url` lives in `dir`.
///
/// The name is a hash of the URL. If the hash changes with a Rust release,
/// the only cost is downloading the images again.
pub fn path_for(dir: &Path, url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let extension = url
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| matches!(*ext, "jpg" | "jpeg" | "png" | "webp"))
        .unwrap_or("jpg");
    dir.join(format!("{:016x}.{extension}", hasher.finish()))
}

/// Writes atomically (temp file + rename) so a crash never leaves half an image.
pub fn store(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("part");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(partial, path)
}

/// Bytes the cached covers take. A missing folder is empty.
pub fn disk_usage(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

/// Removes every cached file not in `keep`. Returns the bytes left and whether
/// everything else was removed (failures are logged).
pub fn clear_except(dir: &Path, keep: &HashSet<PathBuf>) -> (u64, bool) {
    let mut complete = true;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() || keep.contains(&path) {
                continue;
            }
            if let Err(error) = std::fs::remove_file(&path) {
                tracing::warn!(%error, ?path, "could not remove cached artwork");
                complete = false;
            }
        }
    }
    (disk_usage(dir), complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cloudrs-art-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn disk_usage_sums_the_files_and_a_missing_folder_is_empty() {
        let dir = temp_dir("usage");
        std::fs::write(dir.join("a.jpg"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("b.jpg"), [0u8; 5]).unwrap();
        assert_eq!(disk_usage(&dir), 15);
        assert_eq!(disk_usage(&dir.join("nowhere")), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clearing_keeps_only_the_files_in_use() {
        let dir = temp_dir("clear");
        std::fs::write(dir.join("keep.jpg"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("drop.jpg"), [0u8; 5]).unwrap();
        let keep = HashSet::from([dir.join("keep.jpg")]);
        assert_eq!(clear_except(&dir, &keep), (10, true));
        assert!(dir.join("keep.jpg").exists());
        assert!(!dir.join("drop.jpg").exists());
        assert_eq!(clear_except(&dir.join("nowhere"), &keep), (0, true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_url_same_file() {
        let dir = Path::new("/cache");
        let a = path_for(dir, "https://i1.sndcdn.com/artworks-a-t300x300.jpg");
        assert_eq!(
            a,
            path_for(dir, "https://i1.sndcdn.com/artworks-a-t300x300.jpg")
        );
        assert_ne!(
            a,
            path_for(dir, "https://i1.sndcdn.com/artworks-b-t300x300.jpg")
        );
        assert_eq!(a.extension().unwrap(), "jpg");
    }

    #[test]
    fn stores_atomically() {
        let dir = std::env::temp_dir().join(format!("cloudrs-artwork-{}", std::process::id()));
        let path = dir.join("x.jpg");
        store(&path, b"img").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"img");
        assert!(!path.with_extension("part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
