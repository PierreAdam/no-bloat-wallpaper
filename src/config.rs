use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// How often the scheduler should run `nbw run`.
    #[serde(default = "default_interval")]
    pub interval_hours: u32,
    /// "random" picks any unseen image, "latest" picks the first candidate of the first source.
    #[serde(default = "default_rotation")]
    pub rotation: String,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Short unique name, shown in logs and used in file names.
    pub id: String,
    /// "bing", "nasa-apod", "folder" or "json" (user-defined, see docs/custom-sources.md).
    pub kind: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// bing: market code such as "en-US" or "fr-FR".
    pub market: Option<String>,
    /// nasa-apod: API key (defaults to DEMO_KEY, which is rate-limited).
    pub api_key: Option<String>,
    /// folder: local directory of images.
    pub path: Option<String>,
    /// json: URL returning a JSON array, plus the field names to read.
    pub url: Option<String>,
    pub image_field: Option<String>,
    pub title_field: Option<String>,
    pub license: Option<String>,
}

fn yes() -> bool {
    true
}
fn default_interval() -> u32 {
    6
}
fn default_rotation() -> String {
    "random".into()
}

impl Default for Config {
    fn default() -> Self {
        let src = |id: &str, kind: &str| SourceConfig {
            id: id.into(),
            kind: kind.into(),
            enabled: true,
            market: None,
            api_key: None,
            path: None,
            url: None,
            image_field: None,
            title_field: None,
            license: None,
        };
        let mut bing = src("bing", "bing");
        bing.market = Some("en-US".into());
        let mut nasa = src("nasa", "nasa-apod");
        nasa.api_key = Some("DEMO_KEY".into());
        Config {
            interval_hours: default_interval(),
            rotation: default_rotation(),
            sources: vec![bing, nasa],
        }
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

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        save(&Config::default())?;
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, toml::to_string_pretty(cfg)?)?;
    Ok(())
}
