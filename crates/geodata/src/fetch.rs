//! HTTP with an on-disk cache.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const USER_AGENT: &str = concat!(
    "Lifeline/",
    env!("CARGO_PKG_VERSION"),
    " (resupply planner; https://github.com/sekaikx/Trajectory-and-Airdrop-Simulator)"
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
            .timeout(Duration::from_secs(120))
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
            let tmp = dir.join(format!("{key:016x}.part"));
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(tmp, dir.join(format!("{key:016x}")));
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
