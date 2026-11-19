use crate::error::Coded;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Longest interval the scheduler accepts from the settings: one week.
pub const MAX_INTERVAL_HOURS: u32 = 168;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// How often the scheduler runs, in hours (1 to 168).
    #[serde(default = "default_interval")]
    pub interval_hours: u32,
    #[serde(default)]
    pub rotation: Rotation,
    /// After you log in, wait `login_delay_secs`, then refresh the wallpaper if it is overdue
    /// (for example the PC was off at the scheduled time). Does nothing if not overdue.
    #[serde(default = "yes")]
    pub check_on_login: bool,
    #[serde(default = "default_login_delay")]
    pub login_delay_secs: u32,
    /// Let the scheduled tasks start (and keep running) while a laptop is on battery.
    #[serde(default = "yes")]
    pub run_on_battery: bool,
    /// How long a source's answer (and the previews built from it) is reused before the
    /// source is queried again. 0 disables the cache.
    #[serde(default = "default_cache_minutes")]
    pub cache_minutes: u32,
    /// Interface language: "auto" (follow the system) or a code such as "fr".
    #[serde(default = "default_language")]
    pub language: String,
    /// Optional tray icon (GUI only). Off by default: the app then has no resident process.
    #[serde(default)]
    pub tray: bool,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

/// How the scheduled runs choose the next wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rotation {
    /// The newest image of the first enabled source (a manual pick stays until that source
    /// has a newer image).
    #[default]
    Latest,
    /// Any image not shown before.
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SourceKind {
    /// Bing's daily image (built in: at most one).
    #[serde(rename = "bing")]
    Bing,
    /// NASA's Astronomy Picture of the Day (built in: at most one).
    #[serde(rename = "nasa-apod")]
    NasaApod,
    /// Wikimedia Commons' Picture of the Day (built in: at most one).
    #[serde(rename = "wikimedia-potd")]
    WikimediaPotd,
    /// ESA/Webb's Picture of the Month (built in: at most one).
    #[serde(rename = "esa-webb")]
    EsaWebb,
    /// A local directory of images.
    #[default]
    #[serde(rename = "folder")]
    Folder,
    /// A user-defined URL returning JSON.
    #[serde(rename = "json")]
    Json,
}

impl SourceKind {
    /// Remote sources are worth caching; local folders are cheap to read.
    pub fn is_remote(self) -> bool {
        !matches!(self, SourceKind::Folder)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Short unique name, shown in logs and used in file names.
    pub id: String,
    pub kind: SourceKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// bing: market code such as "en-US" or "fr-FR".
    pub market: Option<String>,
    /// folder: local directory of images.
    pub path: Option<String>,
    /// json: URL returning a JSON array (or an object with an "images" array), plus the
    /// field names to read in each item.
    pub url: Option<String>,
    pub image_field: Option<String>,
    pub title_field: Option<String>,
    /// json: optional field holding a small preview URL (otherwise the full image is used).
    pub thumb_field: Option<String>,
    /// folder and json: license text shown with the images.
    pub license: Option<String>,
}

impl SourceConfig {
    pub fn new(id: &str, kind: SourceKind) -> Self {
        SourceConfig { id: id.into(), kind, enabled: true, ..Default::default() }
    }
}

fn yes() -> bool {
    true
}
fn default_interval() -> u32 {
    6
}
fn default_login_delay() -> u32 {
    60
}
fn default_cache_minutes() -> u32 {
    20
}
fn default_language() -> String {
    "auto".into()
}

impl Default for Config {
    fn default() -> Self {
        // Nothing is enabled by default: the user chooses sources in the settings.
        let mut bing = SourceConfig::new("bing", SourceKind::Bing);
        bing.enabled = false;
        bing.market = Some("en-US".into());
        let mut nasa = SourceConfig::new("nasa", SourceKind::NasaApod);
        nasa.enabled = false;
        let mut wikimedia = SourceConfig::new("wikimedia", SourceKind::WikimediaPotd);
        wikimedia.enabled = false;
        let mut webb = SourceConfig::new("webb", SourceKind::EsaWebb);
        webb.enabled = false;
        Config {
            interval_hours: default_interval(),
            rotation: Rotation::default(),
            check_on_login: true,
            login_delay_secs: default_login_delay(),
            run_on_battery: true,
            cache_minutes: default_cache_minutes(),
            language: default_language(),
            tray: false,
            sources: vec![bing, nasa, wikimedia, webb],
        }
    }
}

impl Config {
    /// Check what the program relies on: unique, non-empty source names and at most one of
    /// each built-in source.
    pub fn validate(&self) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        for s in &self.sources {
            let id = s.id.trim();
            if id.is_empty() || !ids.insert(id) {
                return Err(Coded::new(
                    "errNameInvalid",
                    format!("every source needs a unique, non-empty name (found \"{}\")", s.id),
                )
                .into());
            }
        }
        for kind in [SourceKind::Bing, SourceKind::NasaApod, SourceKind::WikimediaPotd, SourceKind::EsaWebb] {
            if self.sources.iter().filter(|s| s.kind == kind).count() > 1 {
                anyhow::bail!("only one {kind:?} source is allowed");
            }
        }
        Ok(())
    }

    /// Values outside what the scheduler accepts are brought back into range.
    fn normalize(&mut self) {
        self.interval_hours = self.interval_hours.clamp(1, MAX_INTERVAL_HOURS);
        self.login_delay_secs = self.login_delay_secs.min(600);
    }
}

pub fn dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "no-bloat-wallpaper").context("cannot determine home directory")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(dirs()?.config_dir().join("config.toml"))
}
pub fn data_dir() -> Result<PathBuf> {
    Ok(dirs()?.data_dir().to_path_buf())
}

/// The settings, creating the default file the first time. A file that cannot be read or
/// is invalid fails with the code "errConfigInvalid" (the GUI then offers to reset it).
pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        save(&Config::default())?;
    }
    parse(&path).map_err(|e| Coded::new("errConfigInvalid", format!("{}: {e:#}", path.display())).into())
}

fn parse(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).context("reading")?;
    let mut cfg: Config = toml::from_str(&text).context("parsing")?;
    cfg.normalize();
    cfg.validate()?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let mut cfg = cfg.clone();
    cfg.normalize();
    cfg.validate()?;
    write_atomic(&config_path()?, toml::to_string_pretty(&cfg)?.as_bytes())
}

/// Replace an unusable config file with the defaults, keeping the old one as
/// `config.toml.bak`. Returns the path of the copy.
pub fn reset() -> Result<PathBuf> {
    let path = config_path()?;
    let backup = path.with_extension("toml.bak");
    if path.exists() {
        std::fs::copy(&path, &backup).with_context(|| format!("copying {}", path.display()))?;
    }
    save(&Config::default())?;
    Ok(backup)
}

/// Write to a temporary file, then rename it over the target, so a crash or a concurrent
/// reader never sees a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    // Unique per process and per call, so threads writing the same file never share one.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().context("path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let tmp = PathBuf::from(tmp);
    let res = std::fs::write(&tmp, bytes)
        .with_context(|| format!("writing {}", tmp.display()))
        .and_then(|()| rename_retrying(&tmp, path).with_context(|| format!("replacing {}", path.display())));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// On Windows a rename over a file another program has open (an antivirus scan, the
/// wallpaper being read) fails for a moment: try a few times before giving up.
fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if attempt < 5 && e.kind() == std::io::ErrorKind::PermissionDenied => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            res => return res,
        }
    }
}

/// An exclusive lock on `<data>/<name>`, waiting at most `wait` for whoever holds it.
/// Released when the file is dropped (also if the process dies). Gives up with the code
/// "errBusy" so the caller can say another change is in progress.
pub fn lock_file(name: &str, wait: std::time::Duration) -> Result<std::fs::File> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(name))
        .with_context(|| format!("opening the lock file {name}"))?;
    let deadline = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(Coded::new("errBusy", "another wallpaper change is in progress; try again shortly").into())
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(anyhow::Error::new(e).context("locking")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_parses() {
        // Missing settings take their defaults; kinds and rotation map to enums.
        let cfg: Config =
            toml::from_str("rotation = \"random\"\n[[sources]]\nid = \"nasa\"\nkind = \"nasa-apod\"\n").unwrap();
        assert_eq!(cfg.rotation, Rotation::Random);
        assert_eq!(cfg.sources[0].kind, SourceKind::NasaApod);
        assert!(cfg.run_on_battery);
    }

    #[test]
    fn duplicate_or_empty_names_are_rejected() {
        let mut cfg = Config::default();
        assert!(cfg.validate().is_ok());
        cfg.sources.push(SourceConfig::new("bing", SourceKind::Folder));
        assert!(cfg.validate().is_err());
        cfg.sources.last_mut().unwrap().id = " ".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn only_one_bing() {
        let mut cfg = Config::default();
        cfg.sources.push(SourceConfig::new("bing2", SourceKind::Bing));
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn atomic_writes_replace_the_file_and_leave_no_temporary() {
        let dir = std::env::temp_dir().join(format!("nbw-test-atomic-{}", std::process::id()));
        let file = dir.join("x.txt");
        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_writers_of_one_file_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("nbw-test-concurrent-{}", std::process::id()));
        let file = dir.join("x.txt");
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| write_atomic(&file, b"same").unwrap());
            }
        });
        assert_eq!(std::fs::read(&file).unwrap(), b"same");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interval_is_clamped() {
        let mut cfg = Config { interval_hours: 0, ..Config::default() };
        cfg.normalize();
        assert_eq!(cfg.interval_hours, 1);
        cfg.interval_hours = 1000;
        cfg.normalize();
        assert_eq!(cfg.interval_hours, MAX_INTERVAL_HOURS);
    }
}
