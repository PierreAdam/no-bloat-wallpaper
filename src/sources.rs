use crate::config::SourceConfig;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// One wallpaper candidate offered by a source. Nothing is downloaded until it is chosen.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Stable id, used for the "already seen" history.
    pub id: String,
    pub title: String,
    pub credit: Option<String>,
    pub license: String,
    pub location: Location,
}

#[derive(Debug, Clone)]
pub enum Location {
    Url(String),
    File(PathBuf),
}

pub fn fetch(src: &SourceConfig) -> Result<Vec<Candidate>> {
    match src.kind.as_str() {
        "bing" => bing(src),
        "nasa-apod" => nasa(src),
        "folder" => folder(src),
        "json" => custom_json(src),
        other => bail!("unknown source kind '{other}'"),
    }
}

fn get_json<T: for<'a> Deserialize<'a>>(url: &str) -> Result<T> {
    ureq::get(url)
        .set("User-Agent", "no-bloat-wallpaper")
        .call()
        .with_context(|| format!("GET {url}"))?
        .into_json()
        .context("decoding JSON")
}

// ---- Bing (unofficial public endpoint) ----

#[derive(Deserialize)]
struct BingResponse {
    images: Vec<BingImage>,
}
#[derive(Deserialize)]
struct BingImage {
    urlbase: String,
    title: String,
    copyright: String,
    hsh: String,
}

fn bing(src: &SourceConfig) -> Result<Vec<Candidate>> {
    let mkt = src.market.as_deref().unwrap_or("en-US");
    let url = format!("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt={mkt}");
    let resp: BingResponse = get_json(&url)?;
    Ok(resp
        .images
        .into_iter()
        .map(|i| Candidate {
            id: format!("bing-{}", i.hsh),
            title: i.title,
            credit: Some(i.copyright),
            license: "Copyright of the credited owner; Bing homepage image. Personal use only unless stated otherwise.".into(),
            location: Location::Url(format!("https://www.bing.com{}_UHD.jpg", i.urlbase)),
        })
        .collect())
}

// ---- NASA Astronomy Picture of the Day ----

#[derive(Deserialize)]
struct Apod {
    media_type: String,
    title: String,
    date: String,
    url: String,
    hdurl: Option<String>,
    copyright: Option<String>,
}

fn nasa(src: &SourceConfig) -> Result<Vec<Candidate>> {
    let key = src.api_key.as_deref().unwrap_or("DEMO_KEY");
    let url = format!("https://api.nasa.gov/planetary/apod?api_key={key}&count=6");
    let items: Vec<Apod> = get_json(&url)?;
    Ok(items
        .into_iter()
        .filter(|a| a.media_type == "image")
        .map(|a| Candidate {
            id: format!("nasa-{}", a.date),
            title: a.title,
            license: if a.copyright.is_some() {
                "Copyrighted by the credited author; check before reuse.".into()
            } else {
                "NASA media, generally public domain (verify on apod.nasa.gov).".into()
            },
            credit: a.copyright,
            location: Location::Url(a.hdurl.unwrap_or(a.url)),
        })
        .collect())
}

// ---- Local folder ----

fn folder(src: &SourceConfig) -> Result<Vec<Candidate>> {
    let dir = src.path.as_deref().context("folder source needs `path`")?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {dir}"))? {
        let path = entry?.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ["jpg", "jpeg", "png", "bmp"].contains(&ext.as_str()) {
            out.push(Candidate {
                id: format!("{}-{}", src.id, path.file_name().unwrap().to_string_lossy()),
                title: path.file_stem().unwrap().to_string_lossy().into_owned(),
                credit: None,
                license: src.license.clone().unwrap_or_else(|| "Your own files.".into()),
                location: Location::File(path),
            });
        }
    }
    Ok(out)
}

// ---- User-defined JSON endpoint ----
// Expects a JSON array of objects (or an object with an "images" array).

fn custom_json(src: &SourceConfig) -> Result<Vec<Candidate>> {
    let url = src.url.as_deref().context("json source needs `url`")?;
    let img_field = src.image_field.as_deref().unwrap_or("url");
    let title_field = src.title_field.as_deref().unwrap_or("title");
    let value: serde_json::Value = get_json(url)?;
    let arr = match &value {
        serde_json::Value::Array(a) => a.clone(),
        v => v.get("images").and_then(|x| x.as_array()).cloned().unwrap_or_default(),
    };
    Ok(arr
        .iter()
        .filter_map(|item| {
            let image = item.get(img_field)?.as_str()?.to_string();
            let title = item.get(title_field).and_then(|t| t.as_str()).unwrap_or("Untitled");
            Some(Candidate {
                id: format!("{}-{}", src.id, image),
                title: title.into(),
                credit: None,
                license: src.license.clone().unwrap_or_else(|| "Unknown; check the source.".into()),
                location: Location::Url(image),
            })
        })
        .collect())
}
