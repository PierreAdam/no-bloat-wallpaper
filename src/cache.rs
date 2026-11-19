//! On-disk cache, so the sources' servers are not queried again and again.
//!
//!   <data>/cache/feeds/<kind>-<hash>.json   what a source offered, with the time it was fetched
//!   <data>/cache/thumbs/<hash>.jpg          small previews, made once (downloaded, or shrunk
//!                                           from a local or full-size picture)
//!   <data>/images/                          full-size pictures (reused when already downloaded)

use crate::config::{self, SourceConfig};
use crate::http;
use crate::platform;
use crate::sources::{self, Candidate};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Downloaded files older than this are deleted (the current wallpaper is always kept).
const KEEP_DAYS: u64 = 30;

pub(crate) fn now_secs() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// FNV-1a: tiny, stable across runs and platforms (unlike the std hasher), good enough for file names.
pub(crate) fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A short, safe file name (without extension) for an image id. Ids can be anything (a
/// custom endpoint's id holds the whole image URL), so the name is a readable start of the
/// id plus a hash of all of it: never too long for the file system, and two ids never share
/// a name just because their unsafe characters were replaced the same way.
pub fn file_stem(id: &str) -> String {
    let readable: String =
        id.chars().take(40).map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '_' }).collect();
    format!("{readable}-{:016x}", fnv(id))
}

pub fn root() -> Result<PathBuf> {
    Ok(config::data_dir()?.join("cache"))
}

fn sub(name: &str) -> Result<PathBuf> {
    let d = root()?.join(name);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn images_dir() -> Result<PathBuf> {
    let d = config::data_dir()?.join("images");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

#[derive(Serialize, Deserialize)]
struct Entry {
    fetched_at: u64,
    /// Local date of the fetch (days since 1970-01-01). An answer from an earlier day is not
    /// reused, so daily pictures change at local midnight. Older cache files have 0: stale.
    #[serde(default)]
    day: i64,
    items: Vec<Candidate>,
}

/// A cache entry is reusable for `ttl_minutes` after it was fetched.
fn is_fresh(fetched_at: u64, now: u64, ttl_minutes: u32) -> bool {
    now.saturating_sub(fetched_at) < u64::from(ttl_minutes) * 60
}

/// Cache file name: the kind plus a hash of everything that changes what the source returns.
fn key(src: &SourceConfig) -> String {
    let params = format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        src.kind, src.market, src.url, src.image_field, src.title_field, src.thumb_field, src.license
    );
    format!("{:?}-{:016x}", src.kind, fnv(&params)).to_lowercase()
}

/// What a source offers, from the cache when it is recent enough. If the source cannot be
/// reached (offline, rate limit), the last saved answer is used however old it is.
pub fn fetch(src: &SourceConfig, ttl_minutes: u32, force: bool) -> Result<Vec<Candidate>> {
    // Local folders are cheap to read, so only remote sources are cached.
    if ttl_minutes == 0 || !src.kind.is_remote() {
        return sources::fetch(src);
    }
    let path = sub("feeds")?.join(format!("{}.json", key(src)));
    let cached: Option<Entry> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok());

    if !force {
        if let Some(e) = &cached {
            if e.day == platform::local_today() && is_fresh(e.fetched_at, now_secs(), ttl_minutes) {
                return Ok(e.items.clone());
            }
        }
    }
    match sources::fetch(src) {
        Ok(items) => {
            let entry = Entry { fetched_at: now_secs(), day: platform::local_today(), items: items.clone() };
            if let Ok(text) = serde_json::to_string(&entry) {
                let _ = config::write_atomic(&path, text.as_bytes());
            }
            Ok(items)
        }
        Err(e) => match cached {
            Some(old) => {
                eprintln!("[{}] using saved answer: {e:#}", src.id);
                Ok(old.items)
            }
            None => Err(e),
        },
    }
}

/// File extension for a picture's bytes: only JPEG and PNG are accepted for downloads
/// (both work as a Windows wallpaper without conversion).
pub fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        Some("jpg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else {
        None
    }
}

/// Check that a local file really is a picture Windows can use (JPEG, PNG or BMP), not a
/// folder, an empty file or something else wearing a picture's extension.
pub fn check_local_image(path: &Path) -> Result<()> {
    use std::io::Read;
    if !path.is_file() {
        anyhow::bail!("{} is not a file", path.display());
    }
    let mut head = [0u8; 8];
    let n = std::fs::File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    if image_ext(head).is_none() && !head.starts_with(b"BM") {
        anyhow::bail!("{} is not a JPEG, PNG or BMP picture", path.display());
    }
    Ok(())
}

/// Download a picture and check that it really is one (JPEG/PNG) and not a placeholder.
pub fn download_image(url: &str, min_bytes: usize) -> Result<Vec<u8>> {
    let bytes = http::get_bytes(url, http::MAX_IMAGE_BYTES)?;
    if image_ext(&bytes).is_none() {
        anyhow::bail!("not a JPEG/PNG image");
    }
    if bytes.len() < min_bytes {
        anyhow::bail!("image too small ({} bytes), likely a placeholder", bytes.len());
    }
    Ok(bytes)
}

/// Longest side of a preview in a strip of previews or in the favorites grid, in pixels.
pub const THUMB_SIZE: u32 = 480;
/// Longest side of the large picture of the wallpaper in use, in pixels: sharp in the
/// window even at 200 % display scaling, for a fraction of a 4K picture's memory.
pub const DISPLAY_SIZE: u32 = 1280;

/// A JPEG copy of a picture no larger than `size` pixels, or None when it already fits (or
/// cannot be decoded). Decoding is bounded, so a hostile file cannot exhaust the memory.
#[cfg(feature = "thumbnails")]
fn shrink(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(limits);
    let img = reader.decode().ok()?;
    if img.width() <= size && img.height() <= size {
        return None;
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.thumbnail(size, size).to_rgb8().write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}
#[cfg(not(feature = "thumbnails"))]
fn shrink(_bytes: &[u8], _size: u32) -> Option<Vec<u8>> {
    None
}

/// A preview image on disk, downloaded (and shrunk) only the first time it is needed.
pub fn thumb(url: &str) -> Option<PathBuf> {
    let file = sub("thumbs").ok()?.join(format!("{:016x}.jpg", fnv(url)));
    if file.metadata().map(|m| m.len() > 1000).unwrap_or(false) {
        return Some(file);
    }
    let bytes = download_image(url, 1000).ok()?;
    config::write_atomic(&file, &shrink(&bytes, THUMB_SIZE).unwrap_or(bytes)).ok()?;
    Some(file)
}

/// Largest picture copied as is when it cannot be shrunk (already small, or not decodable).
const MAX_PREVIEW_COPY: u64 = 3 * 1024 * 1024;

/// A copy of a picture on disk (a local source's file, a download, a favorite) no larger
/// than `size` pixels, kept in the data folder: the interface only ever shows files from
/// there. Made once, and remade when the picture changes.
pub fn local_thumb(path: &Path, size: u32) -> Option<PathBuf> {
    let meta = path.metadata().ok().filter(|m| m.is_file())?;
    let stamp = meta.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    let id = format!("{}|{}|{stamp}|{size}", path.display(), meta.len());
    let file = sub("thumbs").ok()?.join(format!("{:016x}.jpg", fnv(&id)));
    if file.exists() {
        return Some(file);
    }
    let bytes = std::fs::read(path).ok()?;
    let small = match shrink(&bytes, size) {
        Some(small) => small,
        None if meta.len() <= MAX_PREVIEW_COPY => bytes,
        None => return None,
    };
    config::write_atomic(&file, &small).ok()?;
    Some(file)
}

/// Run `f` on every item, a few at a time (never a burst of dozens of requests or decodes).
/// An item whose work panicked (say, a decoder bug on a broken picture) gets `fallback()`
/// instead of bringing the whole program down.
pub fn in_batches<I: Sync, O: Send>(items: &[I], fallback: impl Fn() -> O, f: impl Fn(&I) -> O + Sync) -> Vec<O> {
    let mut out = Vec::with_capacity(items.len());
    for chunk in items.chunks(6) {
        std::thread::scope(|scope| {
            let jobs: Vec<_> = chunk.iter().map(|i| scope.spawn(|| f(i))).collect();
            out.extend(jobs.into_iter().map(|j| j.join().unwrap_or_else(|_| fallback())));
        });
    }
    out
}

/// Delete old downloads: previews and full-size pictures older than `KEEP_DAYS`,
/// except the files in `keep` (the wallpaper in use).
pub fn prune(keep: &[PathBuf]) {
    let limit = Duration::from_secs(KEEP_DAYS * 86_400);
    for dir in [sub("thumbs"), images_dir()].into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if keep.contains(&path) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .map(|age| age > limit)
                .unwrap_or(false);
            if old {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => dir_size(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Bytes used by cached answers, previews and downloaded pictures (favorites are not counted).
pub fn size_bytes() -> u64 {
    let cache = root().map(|d| dir_size(&d)).unwrap_or(0);
    let images = images_dir().map(|d| dir_size(&d)).unwrap_or(0);
    cache + images
}

/// Empty the cache and the downloaded pictures, keeping the files in `keep`.
pub fn clear(keep: &[PathBuf]) -> Result<()> {
    if let Ok(d) = root() {
        let _ = std::fs::remove_dir_all(d);
    }
    if let Ok(dir) = images_dir() {
        for entry in std::fs::read_dir(dir)?.flatten() {
            if !keep.contains(&entry.path()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "thumbnails")]
    use crate::sources::Location;

    #[test]
    fn freshness_window() {
        assert!(is_fresh(1_000, 1_000 + 59 * 60, 60));
        assert!(!is_fresh(1_000, 1_000 + 60 * 60, 60));
        assert!(!is_fresh(1_000, 5_000, 0), "ttl 0 never reuses");
        assert!(is_fresh(5_000, 1_000, 60), "a clock that went backwards still counts as fresh");
    }

    #[test]
    fn hash_is_stable_and_distinguishes() {
        assert_eq!(fnv("https://x/y.jpg"), fnv("https://x/y.jpg"));
        assert_ne!(fnv("https://x/y.jpg"), fnv("https://x/z.jpg"));
        // Known FNV-1a 64 value, so file names stay valid across versions.
        assert_eq!(fnv(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn file_names_are_short_safe_and_distinct() {
        let long = format!("json-https://example.org/{}.jpg", "a".repeat(500));
        let stem = file_stem(&long);
        assert!(stem.len() <= 40 + 17, "{stem}");
        assert!(stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        // The same once unsafe characters are replaced, yet two different names.
        assert_ne!(file_stem("json-https://a.b/c.jpg"), file_stem("json-https://a_b/c_jpg"));
        assert_eq!(file_stem("bing-abc"), file_stem("bing-abc"));
    }

    #[test]
    fn panicking_work_falls_back_instead_of_aborting() {
        let out = in_batches(&[1, 2, 3], || -1, |&n| if n == 2 { panic!("broken picture") } else { n });
        assert_eq!(out, vec![1, -1, 3]);
    }

    #[test]
    fn local_files_must_really_be_pictures() {
        let dir = std::env::temp_dir().join(format!("nbw-test-local-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder.jpg")).unwrap();
        std::fs::write(dir.join("text.jpg"), b"hello").unwrap();
        std::fs::write(dir.join("ok.jpg"), [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
        std::fs::write(dir.join("ok.bmp"), b"BM\0\0").unwrap();
        assert!(check_local_image(&dir.join("folder.jpg")).is_err());
        assert!(check_local_image(&dir.join("text.jpg")).is_err());
        assert!(check_local_image(&dir.join("missing.jpg")).is_err());
        assert!(check_local_image(&dir.join("ok.jpg")).is_ok());
        assert!(check_local_image(&dir.join("ok.bmp")).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hits the network: run with `cargo test -- --ignored`. The preview URL comes from the
    /// current feed, so the test does not depend on one picture staying online.
    #[test]
    #[ignore]
    fn live_thumbnail_is_downloaded_once() {
        let feed = sources::fetch(&SourceConfig::new("nasa", crate::config::SourceKind::NasaApod)).unwrap();
        let url = feed[0].thumb.as_deref().expect("NASA candidates have a preview URL");
        let first = thumb(url).expect("download");
        let stamp = std::fs::metadata(&first).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = thumb(url).expect("cached");
        assert_eq!(first, second);
        assert_eq!(
            stamp,
            std::fs::metadata(&second).unwrap().modified().unwrap(),
            "second call must not rewrite the file"
        );
        assert!(std::fs::metadata(&first).unwrap().len() > 10_000);
    }

    /// Hits the network: `cargo test --features thumbnails -- --ignored`.
    #[cfg(feature = "thumbnails")]
    #[test]
    #[ignore]
    fn live_full_size_picture_becomes_a_small_preview() {
        let feed = sources::fetch(&SourceConfig::new("nasa", crate::config::SourceKind::NasaApod)).unwrap();
        let Location::Url(full) = &feed[0].location else { panic!("expected a URL") };
        let file = thumb(full).expect("preview");
        let len = std::fs::metadata(&file).unwrap().len();
        assert!(len < 150_000, "preview is {len} bytes");
        let img = image::open(&file).unwrap();
        assert!(img.width() <= THUMB_SIZE && img.height() <= THUMB_SIZE);
    }

    #[test]
    fn cache_key_depends_on_source_settings() {
        let mk = |market: &str| SourceConfig {
            market: Some(market.into()),
            ..SourceConfig::new("bing", crate::config::SourceKind::Bing)
        };
        assert_ne!(key(&mk("en-US")), key(&mk("fr-FR")));
        assert_eq!(key(&mk("en-US")), key(&mk("en-US")));
    }
}
