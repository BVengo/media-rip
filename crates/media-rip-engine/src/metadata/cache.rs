use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{Serialize, de::DeserializeOwned};
use sha1::{Digest, Sha1};

const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Debug, Clone)]
pub(super) struct MetadataCache {
    root: Option<PathBuf>,
}

impl MetadataCache {
    pub(super) fn platform_default() -> Self {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".cache"))
            })
            .map(|root| root.join("mediarip").join("metadata"));
        Self { root }
    }

    #[cfg(test)]
    pub(super) fn at(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }

    pub(super) fn read<T: DeserializeOwned>(&self, namespace: &str, key: &str) -> Option<T> {
        self.read_path(namespace, key, false)
    }

    pub(super) fn read_stale<T: DeserializeOwned>(&self, namespace: &str, key: &str) -> Option<T> {
        self.read_path(namespace, key, true)
    }

    fn read_path<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
        allow_stale: bool,
    ) -> Option<T> {
        let path = self.path(namespace, key, "json")?;
        if !allow_stale && cache_age(&path)? > CACHE_MAX_AGE {
            return None;
        }
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    pub(super) fn write<T: Serialize>(&self, namespace: &str, key: &str, value: &T) {
        let Some(path) = self.path(namespace, key, "json") else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        let Ok(data) = serde_json::to_vec(value) else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, data).is_ok() {
            let _ = fs::rename(temporary, path);
        }
    }

    pub(super) fn read_bytes(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        let path = self.path(namespace, key, "bin")?;
        fs::read(path).ok()
    }

    pub(super) fn write_bytes(&self, namespace: &str, key: &str, data: &[u8]) {
        let Some(path) = self.path(namespace, key, "bin") else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let temporary = path.with_extension("bin.tmp");
        if fs::write(&temporary, data).is_ok() {
            let _ = fs::rename(temporary, path);
        }
    }

    fn path(&self, namespace: &str, key: &str, extension: &str) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let digest = Sha1::digest(key.as_bytes());
        Some(root.join(namespace).join(format!("{digest:x}.{extension}")))
    }
}

fn cache_age(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

#[cfg(test)]
mod tests {
    use super::MetadataCache;

    #[test]
    fn round_trips_json_and_binary_cache_entries() {
        let root =
            std::env::temp_dir().join(format!("mediarip-metadata-cache-{}", std::process::id()));
        let cache = MetadataCache::at(root.clone());
        cache.write("discdb", "ABC", &vec!["value"]);
        cache.write_bytes("artwork", "poster", b"image");

        assert_eq!(
            cache.read::<Vec<String>>("discdb", "ABC").unwrap(),
            ["value"]
        );
        assert_eq!(cache.read_bytes("artwork", "poster").unwrap(), b"image");

        let _ = std::fs::remove_dir_all(root);
    }
}
