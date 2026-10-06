//! The artwork cache: one file per artwork URL in the cache folder.

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

#[cfg(test)]
mod tests {
    use super::*;

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
