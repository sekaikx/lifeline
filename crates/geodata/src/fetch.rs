//! HTTP with an on-disk cache.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const USER_AGENT: &str = concat!(
    "Lifeline/",
    env!("CARGO_PKG_VERSION"),
    " (resupply planner; https://github.com/sekaikx/lifeline)"
);

pub struct Fetcher {
    client: reqwest::blocking::Client,
    cache: Option<PathBuf>,
    /// Serve only from the cache; never touch the network.
    pub offline: bool,
}

/// FNV-1a, enough to name cache files.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in *p {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
    }
    h
}

impl Fetcher {
    pub fn new(cache: Option<PathBuf>) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            // longer than the 120 s Overpass server timeout, so slow answers arrive
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|e| e.to_string())?;
        if let Some(dir) = &cache {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create cache {}: {e}", dir.display()))?;
        }
        Ok(Self {
            client,
            cache,
            offline: false,
        })
    }

    /// The platform cache directory, e.g. `~/.cache/lifeline` on Linux.
    pub fn default_cache_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("lifeline"))
    }

    fn cached(&self, key: u64, max_age: Option<Duration>) -> Option<Vec<u8>> {
        let path = self.cache.as_ref()?.join(format!("{key:016x}"));
        let meta = std::fs::metadata(&path).ok()?;
        if let Some(age) = max_age {
            let modified = meta.modified().ok()?;
            let fresh = SystemTime::now()
                .duration_since(modified)
                .map(|d| d < age)
                .unwrap_or(false);
            if !fresh && !self.offline {
                return None;
            }
        }
        std::fs::read(path).ok()
    }

    fn store(&self, key: u64, bytes: &[u8]) {
        if let Some(dir) = &self.cache {
            // unique per writer: the CLI and the app share the cache, and two
            // writers on one temp file can publish a mix of both
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let tmp = dir.join(format!("{key:016x}.{}-{n}.part", std::process::id()));
            if std::fs::write(&tmp, bytes).is_err()
                || std::fs::rename(&tmp, dir.join(format!("{key:016x}"))).is_err()
            {
                let _ = std::fs::remove_file(tmp);
            }
        }
    }

    /// GET `url`, using a cached copy younger than `max_age` (`None`: forever).
    pub fn get(&self, url: &str, max_age: Option<Duration>) -> Result<Vec<u8>, String> {
        let key = fnv(&[b"GET", url.as_bytes()]);
        if let Some(b) = self.cached(key, max_age) {
            return Ok(b);
        }
        if self.offline {
            return Err(format!("offline and not cached: {url}"));
        }
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("{}: {e}", host(url)))?;
        let bytes = check(resp, url)?;
        self.store(key, &bytes);
        Ok(bytes)
    }

    /// POST `body` to `url` with a content type; cached like [`Fetcher::get`].
    pub fn post(
        &self,
        url: &str,
        content_type: &str,
        body: &str,
        max_age: Option<Duration>,
    ) -> Result<Vec<u8>, String> {
        let key = fnv(&[b"POST", url.as_bytes(), body.as_bytes()]);
        if let Some(b) = self.cached(key, max_age) {
            return Ok(b);
        }
        if self.offline {
            return Err(format!("offline and not cached: {url}"));
        }
        let resp = self
            .client
            .post(url)
            .header("content-type", content_type)
            .body(body.to_owned())
            .send()
            .map_err(|e| format!("{}: {e}", host(url)))?;
        let bytes = check(resp, url)?;
        self.store(key, &bytes);
        Ok(bytes)
    }
}

impl Fetcher {
    fn forget(&self, key: u64) {
        if let Some(dir) = &self.cache {
            let _ = std::fs::remove_file(dir.join(format!("{key:016x}")));
        }
    }

    /// Drop the cached answer to a GET, e.g. one that turned out unusable
    /// (an HTML error page served with status 200 by a proxy).
    pub(crate) fn forget_get(&self, url: &str) {
        self.forget(fnv(&[b"GET", url.as_bytes()]));
    }

    /// Drop the cached answer to a POST.
    pub(crate) fn forget_post(&self, url: &str, body: &str) {
        self.forget(fnv(&[b"POST", url.as_bytes(), body.as_bytes()]));
    }
}

fn host(url: &str) -> &str {
    url.split('/').nth(2).unwrap_or(url)
}

fn check(resp: reqwest::blocking::Response, url: &str) -> Result<Vec<u8>, String> {
    let status = resp.status();
    let bytes = resp.bytes().map_err(|e| e.to_string())?.to_vec();
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(300)]).to_string();
        return Err(format!("{} returned {status}: {text}", host(url)));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_without_leftovers_and_forgets() {
        let dir = std::env::temp_dir().join(format!("lifeline-fetch-test-{}", std::process::id()));
        let mut f = Fetcher::new(Some(dir.clone())).unwrap();
        f.offline = true;
        let url = "https://example.invalid/tile.png";
        f.store(fnv(&[b"GET", url.as_bytes()]), b"<html>proxy login</html>");
        assert_eq!(f.get(url, None).unwrap(), b"<html>proxy login</html>");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".part")), "{names:?}");
        f.forget_get(url);
        assert!(f.get(url, None).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
