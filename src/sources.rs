use crate::config::{SourceConfig, SourceKind};
use crate::http;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One wallpaper candidate offered by a source. Nothing is downloaded until it is chosen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable id, used for the "already seen" history.
    pub id: String,
    pub title: String,
    pub credit: Option<String>,
    /// Longer text about the picture, when the source provides one (NASA does, Bing does not).
    pub description: Option<String>,
    /// License text in English (a built-in text, or the user's own note).
    pub license: String,
    /// Translation key of a built-in license text (None for the user's own note).
    #[serde(default)]
    pub license_key: Option<String>,
    pub location: Location,
    /// Small preview image URL for the UI (None for local files: use `location`).
    pub thumb: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Location {
    Url(String),
    File(PathBuf),
}

/// Built-in license texts: (translation key for the GUI, English text for the CLI and logs).
pub const LIC_BING: (&str, &str) =
    ("licBing", "Copyright of the credited owner; Bing homepage image. Personal use only unless stated otherwise.");
pub const LIC_NASA_COPYRIGHT: (&str, &str) =
    ("licNasaCopyright", "Copyrighted by the credited author; check before reuse.");
pub const LIC_NASA_PUBLIC: (&str, &str) =
    ("licNasaPublic", "NASA media, generally public domain (verify on the APOD page).");
pub const LIC_OWN: (&str, &str) = ("licOwn", "Your own files.");
pub const LIC_UNKNOWN: (&str, &str) = ("licUnknown", "Unknown; check the source.");

/// License fields of a candidate: a built-in text (with its key), or the user's own note.
fn license(builtin: (&str, &str), user_note: Option<&String>) -> (String, Option<String>) {
    match user_note.filter(|n| !n.trim().is_empty()) {
        Some(note) => (note.clone(), None),
        None => (builtin.1.into(), Some(builtin.0.into())),
    }
}

pub const LIC_ESA_WEBB: (&str, &str) =
    ("licEsaWebb", "CC BY 4.0. Reuse requires the full credit line shown on the image's esawebb.org page.");

pub fn fetch(src: &SourceConfig) -> Result<Vec<Candidate>> {
    match src.kind {
        SourceKind::Bing => bing(src),
        SourceKind::NasaApod => nasa(),
        SourceKind::WikimediaPotd => wikimedia(),
        SourceKind::EsaWebb => esa_webb(),
        SourceKind::Folder => folder(src),
        SourceKind::Json => custom_json(src),
    }
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

fn bing_url(src: &SourceConfig) -> String {
    let mkt = http::encode(src.market.as_deref().unwrap_or("en-US"));
    format!("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt={mkt}")
}

fn bing(src: &SourceConfig) -> Result<Vec<Candidate>> {
    Ok(parse_bing(http::get_json(&bing_url(src))?))
}

fn parse_bing(resp: BingResponse) -> Vec<Candidate> {
    let (license, license_key) = license(LIC_BING, None);
    resp.images
        .into_iter()
        .map(|i| Candidate {
            id: format!("bing-{}", i.hsh),
            title: i.title,
            credit: Some(i.copyright),
            description: None,
            license: license.clone(),
            license_key: license_key.clone(),
            thumb: Some(format!("https://www.bing.com{}_320x240.jpg", i.urlbase)),
            location: Location::Url(format!("https://www.bing.com{}_UHD.jpg", i.urlbase)),
        })
        .collect()
}

// ---- NASA Astronomy Picture of the Day ----

// NASA moved APOD to science.nasa.gov. Its official RSS feed needs no API key, has no rate
// limit, and lists the last ~30 days. In the feed `apod:url` is an article page, NOT an
// image; the picture is `apod:hdurl`.

const APOD_FEED: &str = "https://science.nasa.gov/feed/apod-basic/";

/// Text between `<name>` and `</name>` (the feed's per-item tags are plain and not nested).
fn xml_tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = s.find(&format!("<{name}>"))? + name.len() + 2;
    let end = s[open..].find(&format!("</{name}>"))? + open;
    Some(&s[open..end])
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let decoded = tail.find(';').filter(|&j| j <= 10).and_then(|j| {
            let ent = &tail[1..j];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            ch.map(|c| (c, j))
        });
        match decoded {
            Some((c, j)) => {
                out.push(c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Plain text from a CDATA/HTML snippet such as `<![CDATA[<a href="...">Name</a>]]>`.
fn strip_tags(s: &str) -> String {
    let s = s.replace("<![CDATA[", "").replace("]]>", "");
    let mut text = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    decode_entities(&text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The feed's explanation is HTML that starts with an "Explanation:" label: keep the text only.
fn apod_description(raw: &str) -> String {
    let text = strip_tags(raw);
    text.strip_prefix("Explanation:").unwrap_or(&text).trim().to_string()
}

fn nasa() -> Result<Vec<Candidate>> {
    Ok(parse_apod(&http::get_text(APOD_FEED)?))
}

fn parse_apod(feed: &str) -> Vec<Candidate> {
    feed.split("<item>")
        .skip(1)
        .filter_map(|item| {
            let title = decode_entities(xml_tag(item, "title")?.trim());
            let link = xml_tag(item, "link")?.trim();
            let hd = decode_entities(xml_tag(item, "apod:hdurl")?.trim());
            let base = hd.split('?').next()?.to_string();
            // Videos have no usable image: keep only pictures Windows can use as a wallpaper.
            let ext = base.rsplit('.').next()?.to_lowercase();
            if !["jpg", "jpeg", "png"].contains(&ext.as_str()) {
                return None;
            }
            let slug = link.trim_end_matches('/').rsplit('/').next()?;
            let credit = xml_tag(item, "apod:copyright").map(strip_tags).filter(|c| !c.is_empty());
            let description = xml_tag(item, "apod:explanation").map(apod_description).filter(|d| !d.is_empty());
            let (license, license_key) =
                license(if credit.is_some() { LIC_NASA_COPYRIGHT } else { LIC_NASA_PUBLIC }, None);
            Some(Candidate {
                id: format!("nasa-{slug}"),
                title,
                description,
                license,
                license_key,
                credit,
                // The image host resizes on request: a small preview keeps the UI light.
                thumb: Some(format!("{base}?w=480&fit=clip")),
                location: Location::Url(hd),
            })
        })
        .collect()
}

// ---- Wikimedia Commons Picture of the Day ----

// Each day's picture is the file used by the page `Template:Potd/YYYY-MM-DD` (pages exist ahead
// of time, so only dates up to today, on this computer's clock, are asked for). Two API calls cover the last two weeks:
// one for the file names, one for their sizes, URLs, author and license.
//
// Pictures are always taken from Wikimedia's scaled copies (at one of its standard widths),
// never the originals: those can weigh hundreds of MB, and their server limits automated
// downloads much more strictly.

const COMMONS_API: &str = "https://commons.wikimedia.org/w/api.php?action=query&format=json&formatversion=2";
const POTD_DAYS: i64 = 14;
/// Widths Wikimedia keeps scaled copies at: the largest one that fits the original is used.
const POTD_WIDTHS: [u32; 4] = [3840, 1920, 1280, 960];
/// Width of the preview.
const POTD_THUMB: u32 = 500;

#[derive(Deserialize)]
struct WmResponse {
    query: Option<WmQuery>,
}
#[derive(Deserialize)]
struct WmQuery {
    #[serde(default)]
    pages: Vec<WmPage>,
}
#[derive(Deserialize)]
struct WmPage {
    title: String,
    #[serde(default)]
    images: Vec<WmTitle>,
    #[serde(default)]
    imageinfo: Vec<WmImageInfo>,
}
#[derive(Deserialize)]
struct WmTitle {
    title: String,
}
#[derive(Deserialize)]
struct WmImageInfo {
    mime: String,
    width: u32,
    thumburl: Option<String>,
    #[serde(default)]
    extmetadata: std::collections::HashMap<String, WmMeta>,
}
#[derive(Deserialize)]
struct WmMeta {
    value: serde_json::Value,
}

/// The last `POTD_DAYS` dates, newest first, as "YYYY-MM-DD". Dates follow this computer's
/// clock, so a new day's picture shows from local midnight (the pages exist ahead of time).
fn recent_dates() -> Vec<String> {
    let today = crate::platform::local_today();
    (0..POTD_DAYS)
        .map(|back| {
            let (y, m, d) = crate::platform::civil_date(today - back);
            format!("{y:04}-{m:02}-{d:02}")
        })
        .collect()
}

fn wm_query(params: &str, titles: &[String]) -> Result<Vec<WmPage>> {
    let titles = titles.iter().map(|t| http::encode(t)).collect::<Vec<_>>().join("%7C");
    let resp: WmResponse = http::get_json(&format!("{COMMONS_API}&{params}&titles={titles}"))?;
    Ok(resp.query.map(|q| q.pages).unwrap_or_default())
}

/// Drop the tracking parameters Wikimedia appends to its URLs.
fn without_query(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
}

fn wikimedia() -> Result<Vec<Candidate>> {
    let dates = recent_dates();
    let templates: Vec<String> = dates.iter().map(|d| format!("Template:Potd/{d}")).collect();
    // Without imlimit the API returns only 10 images across all pages (and the rest needs a
    // follow-up request), which would silently drop some days, often today.
    let pages = wm_query("prop=images&imlimit=max", &templates)?;
    // Date -> file, in date order (newest first).
    let files: Vec<(String, String)> = dates
        .iter()
        .filter_map(|d| {
            let page = pages.iter().find(|p| p.title == format!("Template:Potd/{d}"))?;
            Some((d.clone(), page.images.first()?.title.clone()))
        })
        .collect();
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let names: Vec<String> = files.iter().map(|(_, f)| f.clone()).collect();
    let info = wm_query(
        &format!(
            "prop=imageinfo&iiprop=url%7Csize%7Cmime%7Cextmetadata&iiurlwidth={POTD_THUMB}&iiextmetadatalanguage=en\
             &iiextmetadatafilter=Artist%7CLicenseShortName%7CImageDescription%7CObjectName"
        ),
        &names,
    )?;

    Ok(files
        .into_iter()
        .filter_map(|(date, file)| {
            let page = info.iter().find(|p| p.title == file)?;
            let ii = page.imageinfo.first()?;
            // Photographs only (Wikimedia serves JPEG versions of TIFF files); no video or animation.
            if !["image/jpeg", "image/png", "image/tiff"].contains(&ii.mime.as_str()) {
                return None;
            }
            // The preview's URL names its width: the same URL with a larger width is the
            // picture to download. Too small for a screen (under 960 px): skipped.
            let thumb = without_query(ii.thumburl.as_deref()?);
            let width = POTD_WIDTHS.into_iter().find(|w| *w <= ii.width)?;
            let marker = format!("/{POTD_THUMB}px-");
            if !thumb.contains(&marker) {
                return None;
            }
            let full = thumb.replace(&marker, &format!("/{width}px-"));
            let meta = |k: &str| {
                ii.extmetadata.get(k).and_then(|m| m.value.as_str()).map(strip_tags).filter(|s| !s.is_empty())
            };
            let name = file.trim_start_matches("File:");
            let title = meta("ObjectName")
                .unwrap_or_else(|| name.rsplit_once('.').map_or(name, |(stem, _)| stem).replace('_', " "));
            let (license, license_key) = match meta("LicenseShortName") {
                Some(l) => (format!("{l} (see the file page on Wikimedia Commons)"), None),
                None => license(LIC_UNKNOWN, None),
            };
            Some(Candidate {
                id: format!("wikimedia-{date}"),
                title,
                credit: meta("Artist"),
                description: meta("ImageDescription"),
                license,
                license_key,
                thumb: Some(thumb),
                location: Location::Url(full),
            })
        })
        .collect())
}

// ---- ESA/Webb Picture of the Month ----

// The official RSS feed lists the last 25 pictures of the month. Each one is published in
// several sizes under the same name; the 4000-pixel "publication" JPEG suits a screen.

const WEBB_FEED: &str = "https://esawebb.org/images/potm/feed/";
const WEBB_IMAGES: &str = "https://cdn.esawebb.org/archives/images";

fn esa_webb() -> Result<Vec<Candidate>> {
    Ok(parse_webb(&http::get_text(WEBB_FEED)?))
}

fn parse_webb(feed: &str) -> Vec<Candidate> {
    let (license, license_key) = license(LIC_ESA_WEBB, None);
    feed.split("<item>")
        .skip(1)
        .filter_map(|item| {
            let title = decode_entities(xml_tag(item, "title")?.trim());
            // ".../images/potm2610a/" -> "potm2610a"
            let link = xml_tag(item, "link")?.trim();
            let name = link.trim_end_matches('/').rsplit('/').next()?;
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return None;
            }
            Some(Candidate {
                id: format!("esawebb-{name}"),
                title,
                credit: Some(format!("ESA/Webb · esawebb.org/images/{name}")),
                description: None,
                license: license.clone(),
                license_key: license_key.clone(),
                thumb: Some(format!("{WEBB_IMAGES}/thumb700x/{name}.jpg")),
                location: Location::Url(format!("{WEBB_IMAGES}/publicationjpg/{name}.jpg")),
            })
        })
        .collect()
}

// ---- Local folder ----

fn folder(src: &SourceConfig) -> Result<Vec<Candidate>> {
    let dir = src.path.as_deref().context("folder source needs `path`")?;
    let (license, license_key) = license(LIC_OWN, src.license.as_ref());
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {dir}"))? {
        let entry = entry?;
        // Files only: a sub-folder named "x.jpg" is not a picture.
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let (Some(name), Some(stem)) = (path.file_name(), path.file_stem()) else { continue };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ["jpg", "jpeg", "png", "bmp"].contains(&ext.as_str()) {
            out.push(Candidate {
                id: format!("{}-{}", src.id, name.to_string_lossy()),
                title: stem.to_string_lossy().into_owned(),
                credit: None,
                description: None,
                license: license.clone(),
                license_key: license_key.clone(),
                thumb: None,
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
    Ok(parse_custom_json(src, http::get_json(url)?))
}

fn parse_custom_json(src: &SourceConfig, value: serde_json::Value) -> Vec<Candidate> {
    let img_field = src.image_field.as_deref().unwrap_or("url");
    let title_field = src.title_field.as_deref().unwrap_or("title");
    let thumb_field = src.thumb_field.as_deref().filter(|f| !f.is_empty());
    let (license, license_key) = license(LIC_UNKNOWN, src.license.as_ref());
    let arr = match value {
        serde_json::Value::Array(a) => a,
        mut v => match v.get_mut("images").map(serde_json::Value::take) {
            Some(serde_json::Value::Array(a)) => a,
            _ => Vec::new(),
        },
    };
    arr.iter()
        .filter_map(|item| {
            let image = item.get(img_field)?.as_str()?.to_string();
            let title = item.get(title_field).and_then(|t| t.as_str()).unwrap_or("Untitled");
            // A dedicated preview when the endpoint has one; otherwise the full image (the GUI
            // downloads it once and keeps a small copy).
            let thumb =
                thumb_field.and_then(|f| item.get(f)?.as_str()).map(String::from).unwrap_or_else(|| image.clone());
            Some(Candidate {
                id: format!("{}-{}", src.id, image),
                title: title.into(),
                credit: None,
                description: None,
                license: license.clone(),
                license_key: license_key.clone(),
                thumb: Some(thumb),
                location: Location::Url(image),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bing_answer_becomes_candidates() {
        let resp: BingResponse = serde_json::from_str(
            r#"{"images":[{"urlbase":"/th?id=OHR.Fox_EN-US123","title":"A fox","copyright":"© Someone","hsh":"abc"}]}"#,
        )
        .unwrap();
        let list = parse_bing(resp);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "bing-abc");
        assert_eq!(list[0].credit.as_deref(), Some("© Someone"));
        assert_eq!(list[0].license_key.as_deref(), Some(LIC_BING.0));
        let Location::Url(url) = &list[0].location else { panic!() };
        assert_eq!(url, "https://www.bing.com/th?id=OHR.Fox_EN-US123_UHD.jpg");
    }

    #[test]
    fn bing_market_is_encoded() {
        let mut src = SourceConfig::new("bing", SourceKind::Bing);
        src.market = Some("fr-FR&idx=5".into());
        assert!(bing_url(&src).ends_with("&mkt=fr-FR%26idx%3D5"), "{}", bing_url(&src));
    }

    #[test]
    fn apod_feed_keeps_pictures_and_skips_videos() {
        let feed = r#"<rss><channel><title>APOD</title>
            <item><title>Solar &amp; Flare</title><link>https://science.nasa.gov/apod/solar-flare/</link>
              <apod:hdurl>https://x.nasa.gov/a/flare.jpg?w=3000&#038;h=2000</apod:hdurl>
              <apod:copyright><![CDATA[<a href="x">Jane  Doe</a>]]></apod:copyright>
              <apod:explanation><![CDATA[<strong>Explanation:</strong> Hot.]]></apod:explanation></item>
            <item><title>A video</title><link>https://science.nasa.gov/apod/video/</link>
              <apod:hdurl>https://x.nasa.gov/a/clip.mp4</apod:hdurl></item>
            <item><title>Public</title><link>https://science.nasa.gov/apod/public/</link>
              <apod:hdurl>https://x.nasa.gov/a/pub.PNG</apod:hdurl></item>
            </channel></rss>"#;
        let list = parse_apod(feed);
        assert_eq!(list.len(), 2, "the video is skipped");
        assert_eq!(list[0].id, "nasa-solar-flare");
        assert_eq!(list[0].title, "Solar & Flare");
        assert_eq!(list[0].credit.as_deref(), Some("Jane Doe"));
        assert_eq!(list[0].description.as_deref(), Some("Hot."));
        assert_eq!(list[0].license_key.as_deref(), Some(LIC_NASA_COPYRIGHT.0));
        assert_eq!(list[0].thumb.as_deref(), Some("https://x.nasa.gov/a/flare.jpg?w=480&fit=clip"));
        assert_eq!(list[1].license_key.as_deref(), Some(LIC_NASA_PUBLIC.0), "no credit: public domain");
    }

    #[test]
    fn webb_feed_builds_image_urls_from_safe_names_only() {
        let feed = r#"<rss><channel><title>POTM</title>
            <item><title>Pillars</title><link>https://esawebb.org/images/potm2610a/</link></item>
            <item><title>Odd</title><link>https://esawebb.org/images/../evil?x=1/</link></item>
            </channel></rss>"#;
        let list = parse_webb(feed);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "esawebb-potm2610a");
        let Location::Url(url) = &list[0].location else { panic!() };
        assert_eq!(url, "https://cdn.esawebb.org/archives/images/publicationjpg/potm2610a.jpg");
    }

    #[test]
    fn custom_json_reads_the_configured_fields() {
        let mut src = SourceConfig::new("mine", SourceKind::Json);
        src.image_field = Some("src".into());
        src.thumb_field = Some("small".into());
        src.license = Some("CC0".into());
        let value = serde_json::json!({ "images": [
            { "src": "https://e.org/1.jpg", "title": "One", "small": "https://e.org/1s.jpg" },
            { "src": "https://e.org/2.jpg" },
            { "title": "no image" }
        ]});
        let list = parse_custom_json(&src, value);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "mine-https://e.org/1.jpg");
        assert_eq!(list[0].thumb.as_deref(), Some("https://e.org/1s.jpg"));
        assert_eq!(list[0].license, "CC0");
        assert_eq!(list[0].license_key, None, "the user's own license note");
        assert_eq!(list[1].title, "Untitled");
        assert_eq!(list[1].thumb.as_deref(), Some("https://e.org/2.jpg"), "falls back to the full image");
    }

    #[test]
    fn folder_lists_picture_files_only() {
        let dir = std::env::temp_dir().join(format!("nbw-test-folder-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub.jpg")).unwrap();
        std::fs::write(dir.join("a.JPG"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let mut src = SourceConfig::new("pics", SourceKind::Folder);
        src.path = Some(dir.to_string_lossy().into_owned());
        let list = folder(&src).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(list.len(), 1, "the sub-folder and the text file are skipped");
        assert_eq!(list[0].id, "pics-a.JPG");
        assert_eq!(list[0].title, "a");
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            decode_entities("a&#038;b &amp; c &#8217;d &lt;x&gt; &unknown; e&"),
            "a&b & c \u{2019}d <x> &unknown; e&"
        );
        assert_eq!(decode_entities("w=3000&#038;h=2759&#038;fit=clip"), "w=3000&h=2759&fit=clip");
    }

    #[test]
    fn credit_html_becomes_text() {
        let raw = r#"<![CDATA[<a href="https://example.org/x">Mario  Cogo</a> &amp; friends]]>"#;
        assert_eq!(strip_tags(raw), "Mario Cogo & friends");
    }

    #[test]
    fn apod_explanation_is_plain_text() {
        let raw = r#"<![CDATA[<strong>Explanation:</strong> What a <a href="https://x.org">flare</a> for the <em>chromatic</em>!]]>"#;
        assert_eq!(apod_description(raw), "What a flare for the chromatic!");
    }

    #[test]
    fn tags_are_extracted() {
        let item = "<title>A &amp; B</title><apod:hdurl>https://x/y.jpg?w=1&#038;h=2</apod:hdurl>";
        assert_eq!(xml_tag(item, "title"), Some("A &amp; B"));
        assert_eq!(xml_tag(item, "apod:hdurl"), Some("https://x/y.jpg?w=1&#038;h=2"));
        assert_eq!(xml_tag(item, "missing"), None);
    }

    /// Hits the network: `cargo test -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_wikimedia_potd() {
        // Every day must come back with its file (the API's default limit used to drop some).
        let templates: Vec<String> = recent_dates().iter().map(|d| format!("Template:Potd/{d}")).collect();
        let pages = wm_query("prop=images&imlimit=max", &templates).unwrap();
        let with_file = pages.iter().filter(|p| !p.images.is_empty()).count();
        assert_eq!(with_file, POTD_DAYS as usize, "days without a file");

        let list = wikimedia().unwrap();
        assert!(list.len() >= 10, "only {} pictures", list.len());
        let first = &list[0];
        assert!(first.id.starts_with("wikimedia-"));
        assert!(first.credit.is_some() && !first.license.is_empty());
        let Location::Url(url) = &first.location else { panic!() };
        let bytes = crate::cache::download_image(url, 50_000).unwrap();
        assert!(crate::cache::image_ext(&bytes).is_some());
    }

    #[test]
    #[ignore]
    fn live_esa_webb() {
        let list = esa_webb().unwrap();
        assert!(list.len() >= 10, "only {} pictures", list.len());
        let Location::Url(url) = &list[0].location else { panic!() };
        assert!(crate::cache::download_image(url, 50_000).is_ok());
        assert!(crate::cache::download_image(list[0].thumb.as_ref().unwrap(), 1000).is_ok());
    }
}
