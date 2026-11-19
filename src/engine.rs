//! The shared engine used by both the CLI and the GUI.

use crate::config::{self, Config, Rotation, SourceConfig};
use crate::error::Coded;
use crate::sources::{Candidate, Location};
use crate::{cache, error, platform};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How many ids the "already shown" history keeps (the oldest are dropped first).
const MAX_SEEN: usize = 500;

/// How long a click in the app waits for a change already in progress before saying so.
const INTERACTIVE_WAIT: Duration = Duration::from_secs(15);
/// How long a scheduled run waits (the scheduler stops a run after 10 minutes).
const SCHEDULED_WAIT: Duration = Duration::from_secs(5 * 60);

/// What we remember between runs.
#[derive(Default, Serialize, Deserialize)]
struct State {
    seen: Vec<String>,
    current: Option<Current>,
    /// Unix seconds of the last wallpaper change.
    #[serde(default)]
    last_run: Option<u64>,
    /// Set when the user picked an image by hand: it stays until the source has a new latest image.
    #[serde(default)]
    pin: Option<Pin>,
}

/// A manual pick. `latest_id` is the newest image of `source` at the time of the pick; once the
/// source's newest image is a different one, the pick has served its purpose.
#[derive(Clone, Serialize, Deserialize)]
struct Pin {
    source: String,
    latest_id: String,
}

/// What a scheduled run did.
pub enum Outcome {
    Changed(Current),
    /// Nothing to do, with the reason (written to the log).
    Skipped(String),
}

/// An image that was applied (or saved as a favorite), with its attribution.
#[derive(Clone, Serialize, Deserialize)]
pub struct Current {
    pub id: String,
    pub title: String,
    pub credit: Option<String>,
    /// Longer text about the picture when the source provides one.
    #[serde(default)]
    pub description: Option<String>,
    /// License text in English.
    pub license: String,
    /// Translation key of a built-in license text (none for the user's own license notes).
    #[serde(default)]
    pub license_key: Option<String>,
    pub file: PathBuf,
}

impl Current {
    fn new(pick: Candidate, file: PathBuf) -> Self {
        Current {
            id: pick.id,
            title: pick.title,
            credit: pick.credit,
            description: pick.description,
            license: pick.license,
            license_key: pick.license_key,
            file,
        }
    }
}

fn no_source() -> anyhow::Error {
    Coded::new("errNoSource", "No source is enabled. Open the settings and enable at least one.").into()
}

// ---- The outside world, behind a trait so the decisions below can be tested ----

/// Everything the wallpaper logic needs from outside: the sources, the downloads, the
/// desktop and the clock.
trait World {
    fn fetch(&self, src: &SourceConfig, ttl_minutes: u32, force: bool) -> Result<Vec<Candidate>>;
    /// A local file for the candidate (downloaded if needed), checked to be a usable picture.
    fn materialize(&self, c: &Candidate) -> Result<PathBuf>;
    fn set_wallpaper(&self, file: &Path) -> Result<()>;
    fn now(&self) -> u64;
}

/// The real thing: network, disk and the OS.
struct Real;

impl World for Real {
    fn fetch(&self, src: &SourceConfig, ttl_minutes: u32, force: bool) -> Result<Vec<Candidate>> {
        cache::fetch(src, ttl_minutes, force)
    }
    fn materialize(&self, c: &Candidate) -> Result<PathBuf> {
        materialize(c)
    }
    fn set_wallpaper(&self, file: &Path) -> Result<()> {
        platform::set_wallpaper(file)
    }
    fn now(&self) -> u64 {
        cache::now_secs()
    }
}

// ---- State on disk, and the lock that serialises changes to it ----

fn state_path() -> Result<PathBuf> {
    Ok(config::data_dir()?.join("state.json"))
}

fn load_state() -> State {
    state_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(s: &State) -> Result<()> {
    config::write_atomic(&state_path()?, serde_json::to_string_pretty(s)?.as_bytes())
}

/// Run `f` on the settings and the saved state while holding the lock, so the scheduled task
/// and the GUI never overwrite each other's work. The state is saved afterwards (also after a
/// failure: rejected images are remembered), and old downloads are pruned after a change.
fn with_state<T>(wait: Duration, f: impl FnOnce(&Real, &Config, &mut State) -> Result<T>) -> Result<T> {
    let _lock = config::lock_file("nbw.lock", wait)?;
    let cfg = config::load()?;
    let mut state = load_state();
    let before = state.last_run;
    let res = f(&Real, &cfg, &mut state);
    let saved = save_state(&state);
    let out = res?;
    saved?;
    if state.last_run != before {
        if let Some(cur) = &state.current {
            cache::prune(std::slice::from_ref(&cur.file));
        }
    }
    Ok(out)
}

pub fn current() -> Option<Current> {
    load_state().current
}

// ---- Status and maintenance ----

/// Data for the settings screen's "next change" line. `schedule` is what the OS scheduler
/// really has registered, so the countdown reflects reality (None = nothing scheduled).
#[derive(Serialize)]
pub struct Status {
    pub schedule: Option<platform::ScheduleInfo>,
    pub interval_hours: u32,
    /// Whether the after-login check is registered with the OS scheduler.
    pub login_check: bool,
    /// Whether this computer runs on a battery (the "run on battery" option applies).
    pub has_battery: bool,
}

pub fn status() -> Result<Status> {
    Ok(Status {
        schedule: platform::schedule_info(),
        interval_hours: config::load()?.interval_hours,
        login_check: platform::login_check_installed(),
        has_battery: platform::has_battery(),
    })
}

/// Ask every enabled source what it offers, returning (source id, result) pairs. The answers
/// are saved in the source cache; the wallpaper does not change.
pub fn check_sources() -> Result<Vec<(String, Result<Vec<Candidate>>)>> {
    let cfg = config::load()?;
    Ok(cfg
        .sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| (s.id.clone(), cache::fetch(s, cfg.cache_minutes, false)))
        .collect())
}

/// Bytes used by cached answers, previews and downloaded pictures.
pub fn cache_size() -> u64 {
    cache::size_bytes()
}

/// Empty the cache and the downloaded pictures (the wallpaper in use is kept).
pub fn clear_cache() -> Result<()> {
    let _lock = config::lock_file("nbw.lock", INTERACTIVE_WAIT)?;
    let keep: Vec<PathBuf> = load_state().current.map(|c| c.file).into_iter().collect();
    cache::clear(&keep)
}

/// Register everything the scheduler needs according to the config: the periodic task,
/// plus the after-login check when enabled. Returns the interval in hours.
pub fn apply_schedule() -> Result<u32> {
    let cfg = config::load()?;
    let opts = platform::TaskOptions { interval_hours: cfg.interval_hours, run_on_battery: cfg.run_on_battery };
    platform::install_schedule(&opts)?;
    if cfg.check_on_login {
        platform::install_login_check(cfg.login_delay_secs, &opts)?;
    } else {
        platform::remove_login_check();
    }
    Ok(cfg.interval_hours)
}

// ---- Choosing and applying a wallpaper ----

#[derive(Debug, PartialEq)]
enum LatestDecision {
    Apply,
    KeepPick,
    AlreadyLatest,
}

/// "Latest" mode: the wallpaper should be the first source's newest image. A manual pick is
/// kept until that newest image changes; if the newest image is already showing, nothing to do.
fn decide_latest(pin: Option<&Pin>, source_id: &str, newest_id: &str, current_id: Option<&str>) -> LatestDecision {
    if let Some(p) = pin {
        if p.source == source_id && p.latest_id == newest_id {
            return LatestDecision::KeepPick;
        }
    }
    if current_id == Some(newest_id) {
        return LatestDecision::AlreadyLatest;
    }
    LatestDecision::Apply
}

fn first_enabled(cfg: &Config) -> Option<&SourceConfig> {
    cfg.sources.iter().find(|s| s.enabled)
}

/// The pin for a manual pick, so scheduled runs leave it alone until the first source has a
/// new latest image (only meaningful in "latest" mode).
fn manual_pin(w: &dyn World, cfg: &Config) -> Option<Pin> {
    if cfg.rotation != Rotation::Latest {
        return None;
    }
    let src = first_enabled(cfg)?;
    let newest = w.fetch(src, cfg.cache_minutes, false).ok()?.into_iter().next()?;
    Some(Pin { source: src.id.clone(), latest_id: newest.id })
}

/// What the scheduler runs. In "latest" mode it applies the first source's newest image, unless
/// it is already showing or you picked something by hand (that pick stays until the source has
/// a newer image). In "random" mode it picks a random unseen image.
pub fn run_scheduled() -> Result<Outcome> {
    with_state(SCHEDULED_WAIT, |w, cfg, state| scheduled(w, cfg, state))
}

fn scheduled(w: &dyn World, cfg: &Config, state: &mut State) -> Result<Outcome> {
    if cfg.rotation == Rotation::Random {
        return change_to_unseen(w, cfg, state).map(Outcome::Changed);
    }
    let src = first_enabled(cfg).ok_or_else(no_source)?;
    let Some(newest) = w.fetch(src, cfg.cache_minutes, false)?.into_iter().next() else {
        return change_to_unseen(w, cfg, state).map(Outcome::Changed); // the source offers nothing right now
    };
    match decide_latest(state.pin.as_ref(), &src.id, &newest.id, state.current.as_ref().map(|c| c.id.as_str())) {
        LatestDecision::KeepPick => {
            Ok(Outcome::Skipped(format!("your pick stays until \"{}\" has a newer image", src.id)))
        }
        LatestDecision::AlreadyLatest => {
            state.pin = None;
            Ok(Outcome::Skipped("already showing the newest image".into()))
        }
        LatestDecision::Apply => apply(w, state, newest, None).map(Outcome::Changed),
    }
}

/// "Change now". In "latest" mode: ask the first source again and load its newest image right
/// away, even over a manual pick; if that image is already showing, nothing changes. In "random"
/// mode: a random image not shown before.
pub fn refresh_now() -> Result<Outcome> {
    with_state(INTERACTIVE_WAIT, |w, cfg, state| refresh(w, cfg, state))
}

fn refresh(w: &dyn World, cfg: &Config, state: &mut State) -> Result<Outcome> {
    if cfg.rotation == Rotation::Random {
        return change_to_unseen(w, cfg, state).map(Outcome::Changed);
    }
    let src = first_enabled(cfg).ok_or_else(no_source)?;
    let Some(newest) = w.fetch(src, cfg.cache_minutes, true)?.into_iter().next() else {
        return change_to_unseen(w, cfg, state).map(Outcome::Changed); // the source offers nothing right now
    };
    state.pin = None; // an explicit "change now" ends any manual pick
    let current = state.current.as_ref().map(|c| c.id.as_str());
    if decide_latest(None, &src.id, &newest.id, current) == LatestDecision::AlreadyLatest {
        return Ok(Outcome::Skipped("already showing the newest image".into()));
    }
    apply(w, state, newest, None).map(Outcome::Changed)
}

/// After login: catch up on a missed run. In "latest" mode that is simply a check for a newer
/// image; in "random" mode the wallpaper is refreshed only if the last change is older than
/// the interval.
pub fn run_if_due() -> Result<Outcome> {
    with_state(SCHEDULED_WAIT, |w, cfg, state| if_due(w, cfg, state))
}

fn if_due(w: &dyn World, cfg: &Config, state: &mut State) -> Result<Outcome> {
    if cfg.rotation == Rotation::Latest {
        return scheduled(w, cfg, state);
    }
    let every = u64::from(cfg.interval_hours) * 60;
    match state.last_run.map(|t| w.now().saturating_sub(t) / 60) {
        Some(a) if a < every => Ok(Outcome::Skipped(format!("last change {a} min ago, overdue after {every} min"))),
        _ => change_to_unseen(w, cfg, state).map(Outcome::Changed),
    }
}

/// Pick an image not shown before from all enabled sources (the newest first in "latest"
/// mode, a random one in "random" mode) and apply it. When every image has been shown, the
/// history starts over right away.
fn change_to_unseen(w: &dyn World, cfg: &Config, state: &mut State) -> Result<Current> {
    let mut all: Vec<Candidate> = Vec::new();
    let mut first_error = None;
    for src in cfg.sources.iter().filter(|s| s.enabled) {
        match w.fetch(src, cfg.cache_minutes, false) {
            Ok(c) => all.extend(c),
            Err(e) => {
                eprintln!("[{}] skipped: {e:#}", src.id);
                first_error.get_or_insert(e.context(format!("source \"{}\"", src.id)));
            }
        }
    }
    if all.is_empty() {
        // Every source failed (offline?): report the cause so the task can retry.
        return Err(first_error.unwrap_or_else(|| {
            if cfg.sources.iter().any(|s| s.enabled) {
                anyhow::anyhow!("the enabled sources offer no images right now")
            } else {
                no_source()
            }
        }));
    }

    let mut fresh: Vec<Candidate> = all.iter().filter(|c| !state.seen.contains(&c.id)).cloned().collect();
    if fresh.is_empty() {
        // Everything has been shown: start over, only skipping the picture showing now.
        state.seen.clear();
        let showing = state.current.as_ref().map(|c| c.id.clone());
        fresh = all.into_iter().filter(|c| Some(&c.id) != showing.as_ref()).collect();
    }

    // Try candidates until one downloads and is accepted as the wallpaper; remember the bad
    // ones (a broken download, a damaged local file) so they are skipped next time.
    loop {
        if fresh.is_empty() {
            anyhow::bail!("no usable image found this time");
        }
        let pick = match cfg.rotation {
            Rotation::Latest => fresh.remove(0),
            Rotation::Random => fresh.swap_remove(fastrand::usize(..fresh.len())),
        };
        let id = pick.id.clone();
        match apply(w, state, pick, None) {
            Ok(cur) => return Ok(cur),
            Err(e) => {
                eprintln!("[{id}] rejected: {e:#}");
                remember_seen(state, id);
            }
        }
    }
}

fn remember_seen(state: &mut State, id: String) {
    state.seen.push(id);
    if state.seen.len() > MAX_SEEN {
        let extra = state.seen.len() - (MAX_SEEN - 100);
        state.seen.drain(..extra);
    }
}

/// Make `pick` the wallpaper (downloading it if needed) and record it in the state.
fn apply(w: &dyn World, state: &mut State, pick: Candidate, pin: Option<Pin>) -> Result<Current> {
    let file = w.materialize(&pick)?;
    w.set_wallpaper(&file)?;
    remember_seen(state, pick.id.clone());
    let cur = Current::new(pick, file);
    record(w, state, cur.clone(), pin);
    Ok(cur)
}

/// Remember the wallpaper now showing, when it changed, and the manual pick if any.
fn record(w: &dyn World, state: &mut State, cur: Current, pin: Option<Pin>) {
    state.current = Some(cur);
    state.last_run = Some(w.now());
    state.pin = pin;
}

/// Download (or check) the chosen image and return a local file path.
fn materialize(c: &Candidate) -> Result<PathBuf> {
    match &c.location {
        Location::File(p) => {
            cache::check_local_image(p)?;
            Ok(p.clone())
        }
        Location::Url(url) => {
            let dir = cache::images_dir()?;
            let name = cache::file_stem(&c.id);
            // Already downloaded (picked before, or shown as the same id again): reuse it.
            for ext in ["jpg", "png"] {
                let file = dir.join(format!("{name}.{ext}"));
                if file.metadata().map(|m| m.len() >= 50_000).unwrap_or(false) {
                    return Ok(file);
                }
            }
            let bytes = cache::download_image(url, 50_000)?;
            let ext = cache::image_ext(&bytes).unwrap_or("jpg");
            let file = dir.join(format!("{name}.{ext}"));
            config::write_atomic(&file, &bytes)?;
            Ok(file)
        }
    }
}

// ---- Browsing what each source offers, and picking one by hand ----

/// One image a source currently offers, shaped for the UI.
#[derive(Serialize)]
pub struct Offer {
    pub id: String,
    pub title: String,
    pub credit: Option<String>,
    pub license: String,
    pub license_key: Option<String>,
    /// Local file of the preview, or None when it could not be made.
    pub thumb: Option<PathBuf>,
}

#[derive(Serialize)]
pub struct SourceOffers {
    pub id: String,
    pub kind: config::SourceKind,
    pub offers: Vec<Offer>,
    pub error: Option<String>,
    /// Translation key for the error, when it is a known one (e.g. "rateLimited").
    pub error_code: Option<&'static str>,
}

/// What every enabled source offers right now. Answers and previews come from the cache when
/// recent; `force` asks the sources again. Previews are always small local copies in the data
/// folder, so the UI never hotlinks the sources and never loads full-size pictures.
pub fn offers(force: bool) -> Result<Vec<SourceOffers>> {
    let cfg = config::load()?;
    let ttl = cfg.cache_minutes;
    let enabled: Vec<_> = cfg.sources.iter().filter(|s| s.enabled).collect();
    let answers = cache::in_batches(
        &enabled,
        || Err(anyhow::anyhow!("reading this source failed unexpectedly")),
        |s| cache::fetch(s, ttl, force),
    );
    Ok(enabled
        .into_iter()
        .zip(answers)
        .map(|(s, res)| match res {
            Ok(list) => {
                let thumbs = cache::in_batches(
                    &list,
                    || None,
                    |c| match (&c.thumb, &c.location) {
                        (Some(url), _) => cache::thumb(url),
                        (None, Location::File(p)) => cache::local_thumb(p, cache::THUMB_SIZE),
                        (None, Location::Url(url)) => cache::thumb(url),
                    },
                );
                let offers = list
                    .into_iter()
                    .zip(thumbs)
                    .map(|(c, thumb)| Offer {
                        id: c.id,
                        title: c.title,
                        credit: c.credit,
                        license: c.license,
                        license_key: c.license_key,
                        thumb,
                    })
                    .collect();
                SourceOffers { id: s.id.clone(), kind: s.kind, offers, error: None, error_code: None }
            }
            Err(e) => SourceOffers {
                id: s.id.clone(),
                kind: s.kind,
                offers: vec![],
                error_code: error::code_of(&e),
                error: Some(format!("{e:#}")),
            },
        })
        .collect())
}

/// A copy of a picture on disk sized for the interface (`cache::THUMB_SIZE` or
/// `cache::DISPLAY_SIZE`), kept in the data folder. None when it cannot be made.
pub fn preview(file: &Path, size: u32) -> Option<PathBuf> {
    cache::local_thumb(file, size)
}

/// Use a specific image from a source as the wallpaper. In "latest" mode it stays until the
/// first source has a newer image (or you press "Change now"); in "random" mode until the next
/// scheduled refresh.
pub fn apply_offer(source_id: &str, offer_id: &str) -> Result<Current> {
    with_state(INTERACTIVE_WAIT, |w, cfg, state| pick_offer(w, cfg, state, source_id, offer_id))
}

fn pick_offer(w: &dyn World, cfg: &Config, state: &mut State, source_id: &str, offer_id: &str) -> Result<Current> {
    let src = cfg.sources.iter().find(|s| s.id == source_id && s.enabled).context("that source is not enabled")?;
    let pick = w
        .fetch(src, cfg.cache_minutes, false)?
        .into_iter()
        .find(|c| c.id == offer_id)
        .context("that image is no longer offered by the source")?;
    let pin = manual_pin(w, cfg);
    apply(w, state, pick, pin)
}

// ---- Favorites ----

fn favorites_dir() -> Result<PathBuf> {
    let dir = config::data_dir()?.join("favorites");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Copy the current wallpaper and its license info into the favorites folder.
pub fn favorite_current() -> Result<Current> {
    let cur = load_state().current.context("no wallpaper has been set yet")?;
    let dir = favorites_dir()?;
    let ext = cur.file.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    let dest = dir.join(format!("{}.{ext}", cache::file_stem(&cur.id)));
    std::fs::copy(&cur.file, &dest)?;
    let fav = Current { file: dest.clone(), ..cur };
    config::write_atomic(&dest.with_extension("json"), serde_json::to_string_pretty(&fav)?.as_bytes())?;
    Ok(fav)
}

/// The saved favorites, each with the path of its `.json` description.
fn favorites_on_disk() -> Result<Vec<(PathBuf, Current)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(favorites_dir()?)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(fav) = serde_json::from_str::<Current>(&std::fs::read_to_string(&path)?) {
                if fav.file.exists() {
                    out.push((path, fav));
                }
            }
        }
    }
    out.sort_by(|a, b| a.1.title.cmp(&b.1.title));
    Ok(out)
}

pub fn list_favorites() -> Result<Vec<Current>> {
    Ok(favorites_on_disk()?.into_iter().map(|(_, f)| f).collect())
}

/// Set a saved favorite as the current wallpaper. It counts as a change (for the "random"
/// mode's overdue check) and as a manual pick (for "latest" mode).
pub fn apply_favorite(id: &str) -> Result<Current> {
    let fav = list_favorites()?.into_iter().find(|f| f.id == id).context("favorite not found")?;
    with_state(INTERACTIVE_WAIT, |w, cfg, state| {
        w.set_wallpaper(&fav.file)?;
        let pin = manual_pin(w, cfg);
        record(w, state, fav.clone(), pin);
        Ok(fav)
    })
}

/// Delete a favorite: its description, and its picture only when that picture really is in
/// the favorites folder (the description is a plain file and must not name any other file).
pub fn delete_favorite(id: &str) -> Result<()> {
    let dir = favorites_dir()?.canonicalize()?;
    if let Some((json, fav)) = favorites_on_disk()?.into_iter().find(|(_, f)| f.id == id) {
        if is_inside(&fav.file, &dir) {
            std::fs::remove_file(&fav.file)?;
        }
        std::fs::remove_file(&json)?;
    }
    Ok(())
}

/// Whether `file` sits directly in `dir` (already canonical), links and `..` resolved.
fn is_inside(file: &Path, dir: &Path) -> bool {
    file.canonicalize().ok().and_then(|f| f.parent().map(|p| p == dir)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceKind;
    use std::cell::RefCell;
    use std::collections::HashMap;

    fn pin(source: &str, latest: &str) -> Pin {
        Pin { source: source.into(), latest_id: latest.into() }
    }

    #[test]
    fn latest_is_applied_when_something_else_is_showing() {
        assert_eq!(decide_latest(None, "bing", "L2", Some("L1")), LatestDecision::Apply);
        assert_eq!(decide_latest(None, "bing", "L2", None), LatestDecision::Apply);
    }

    #[test]
    fn nothing_to_do_when_the_newest_is_already_showing() {
        assert_eq!(decide_latest(None, "bing", "L1", Some("L1")), LatestDecision::AlreadyLatest);
    }

    #[test]
    fn a_manual_pick_stays_until_the_source_has_a_newer_image() {
        let p = pin("bing", "L1");
        // Same newest image as when the user picked: keep the pick.
        assert_eq!(decide_latest(Some(&p), "bing", "L1", Some("older")), LatestDecision::KeepPick);
        // The source published L2: the pick has served its purpose, apply the new latest.
        assert_eq!(decide_latest(Some(&p), "bing", "L2", Some("older")), LatestDecision::Apply);
    }

    #[test]
    fn a_pin_for_another_source_is_ignored() {
        // Sources were reordered: the pin belonged to a source that is no longer first.
        let p = pin("nasa", "L1");
        assert_eq!(decide_latest(Some(&p), "bing", "L1", Some("older")), LatestDecision::Apply);
    }

    // ---- The whole flow, against a fake world ----

    /// Sources answer from a table (a missing source fails like a network error), pictures
    /// listed in `broken` are refused by the desktop, and the clock is set by hand.
    #[derive(Default)]
    struct Fake {
        offers: HashMap<String, Vec<Candidate>>,
        broken: Vec<String>,
        applied: RefCell<Vec<PathBuf>>,
        now: u64,
    }

    impl World for Fake {
        fn fetch(&self, src: &SourceConfig, _ttl: u32, _force: bool) -> Result<Vec<Candidate>> {
            self.offers.get(&src.id).cloned().context("offline")
        }
        fn materialize(&self, c: &Candidate) -> Result<PathBuf> {
            Ok(PathBuf::from(format!("/pictures/{}", c.id)))
        }
        fn set_wallpaper(&self, file: &Path) -> Result<()> {
            if self.broken.iter().any(|b| file.ends_with(b)) {
                anyhow::bail!("Windows refused {}", file.display());
            }
            self.applied.borrow_mut().push(file.to_path_buf());
            Ok(())
        }
        fn now(&self) -> u64 {
            self.now
        }
    }

    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            title: id.into(),
            credit: None,
            description: None,
            license: "test".into(),
            license_key: None,
            location: Location::Url(format!("https://example.org/{id}.jpg")),
            thumb: None,
        }
    }

    fn world(sources: &[(&str, &[&str])]) -> Fake {
        Fake {
            offers: sources
                .iter()
                .map(|(src, ids)| (src.to_string(), ids.iter().map(|id| candidate(id)).collect()))
                .collect(),
            now: 1_000_000,
            ..Fake::default()
        }
    }

    fn config(rotation: Rotation, sources: &[&str]) -> Config {
        Config {
            rotation,
            sources: sources.iter().map(|id| SourceConfig::new(id, SourceKind::Json)).collect(),
            ..Config::default()
        }
    }

    fn changed(o: Result<Outcome>) -> String {
        match o.unwrap() {
            Outcome::Changed(c) => c.id,
            Outcome::Skipped(why) => panic!("skipped: {why}"),
        }
    }

    fn skipped(o: Result<Outcome>) -> String {
        match o.unwrap() {
            Outcome::Changed(c) => panic!("changed to {}", c.id),
            Outcome::Skipped(why) => why,
        }
    }

    #[test]
    fn latest_mode_applies_the_newest_once() {
        let w = world(&[("a", &["a1", "a0"])]);
        let cfg = config(Rotation::Latest, &["a"]);
        let mut st = State::default();
        assert_eq!(changed(scheduled(&w, &cfg, &mut st)), "a1");
        assert_eq!(st.last_run, Some(1_000_000));
        assert!(skipped(scheduled(&w, &cfg, &mut st)).contains("already showing"));
        assert_eq!(w.applied.borrow().len(), 1);
    }

    #[test]
    fn a_manual_pick_is_kept_until_the_source_publishes_something_new() {
        let mut w = world(&[("a", &["a1", "a0"])]);
        let cfg = config(Rotation::Latest, &["a"]);
        let mut st = State::default();
        pick_offer(&w, &cfg, &mut st, "a", "a0").unwrap();
        assert!(skipped(scheduled(&w, &cfg, &mut st)).contains("your pick stays"));
        assert_eq!(st.current.as_ref().unwrap().id, "a0");
        // The source has a new newest image: the pick has served its purpose.
        w.offers.insert("a".into(), vec![candidate("a2"), candidate("a1")]);
        assert_eq!(changed(scheduled(&w, &cfg, &mut st)), "a2");
        assert!(st.pin.is_none());
    }

    #[test]
    fn change_now_overrides_a_manual_pick() {
        let w = world(&[("a", &["a1", "a0"])]);
        let cfg = config(Rotation::Latest, &["a"]);
        let mut st = State::default();
        pick_offer(&w, &cfg, &mut st, "a", "a0").unwrap();
        assert_eq!(changed(refresh(&w, &cfg, &mut st)), "a1");
        assert!(st.pin.is_none());
    }

    #[test]
    fn a_picture_the_desktop_refuses_is_skipped_and_remembered() {
        let mut w = world(&[("a", &["bad", "good"])]);
        w.broken.push("bad".into());
        let cfg = config(Rotation::Latest, &["a"]);
        let mut st = State::default();
        let cur = change_to_unseen(&w, &cfg, &mut st).unwrap();
        assert_eq!(cur.id, "good", "the run goes on with the next picture");
        assert!(st.seen.contains(&"bad".to_string()), "so it is not tried again");
    }

    #[test]
    fn when_every_picture_failed_the_rejections_are_still_remembered() {
        let mut w = world(&[("a", &["x", "y"])]);
        w.broken = vec!["x".into(), "y".into()];
        let cfg = config(Rotation::Random, &["a"]);
        let mut st = State::default();
        assert!(change_to_unseen(&w, &cfg, &mut st).is_err());
        assert_eq!(st.seen.len(), 2);
    }

    #[test]
    fn history_starts_over_without_repeating_the_current_picture() {
        let w = world(&[("a", &["a1", "a2"])]);
        let cfg = config(Rotation::Random, &["a"]);
        let mut st = State::default();
        let first = change_to_unseen(&w, &cfg, &mut st).unwrap().id;
        let second = change_to_unseen(&w, &cfg, &mut st).unwrap().id;
        assert_ne!(first, second);
        // Both shown: the history resets, and the one showing now is not picked again.
        let third = change_to_unseen(&w, &cfg, &mut st).unwrap().id;
        assert_eq!(third, first);
    }

    #[test]
    fn a_failing_source_does_not_stop_the_others() {
        let w = world(&[("b", &["b1"])]);
        let cfg = config(Rotation::Random, &["a", "b"]); // "a" is offline
        let mut st = State::default();
        assert_eq!(change_to_unseen(&w, &cfg, &mut st).unwrap().id, "b1");
    }

    #[test]
    fn every_source_failing_reports_the_cause() {
        let w = world(&[]);
        let cfg = config(Rotation::Random, &["a"]);
        let err = change_to_unseen(&w, &cfg, &mut State::default()).err().unwrap();
        assert!(format!("{err:#}").contains("offline"), "{err:#}");
    }

    #[test]
    fn no_enabled_source_has_its_own_code() {
        let w = world(&[]);
        let mut cfg = config(Rotation::Latest, &["a"]);
        cfg.sources[0].enabled = false;
        let err = scheduled(&w, &cfg, &mut State::default()).err().unwrap();
        assert_eq!(error::code_of(&err), Some("errNoSource"));
    }

    #[test]
    fn random_mode_after_login_waits_until_overdue() {
        let mut w = world(&[("a", &["a1", "a2"])]);
        let cfg = config(Rotation::Random, &["a"]); // every 6 h by default
        let mut st = State { last_run: Some(w.now - 3600), ..State::default() };
        assert!(skipped(if_due(&w, &cfg, &mut st)).contains("60 min ago"));
        w.now += 6 * 3600;
        changed(if_due(&w, &cfg, &mut st));
    }

    #[test]
    fn the_history_stays_bounded() {
        let mut st = State::default();
        for i in 0..(MAX_SEEN + 1) {
            remember_seen(&mut st, i.to_string());
        }
        assert!(st.seen.len() <= MAX_SEEN);
        assert_eq!(st.seen.last().unwrap(), &MAX_SEEN.to_string(), "the newest are kept");
    }

    #[test]
    fn only_files_directly_in_the_folder_count_as_inside() {
        let dir = std::env::temp_dir().join(format!("nbw-test-inside-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("fav.jpg"), b"x").unwrap();
        std::fs::write(dir.join("sub").join("other.jpg"), b"x").unwrap();
        let canon = dir.canonicalize().unwrap();
        assert!(is_inside(&dir.join("fav.jpg"), &canon));
        assert!(is_inside(&dir.join("sub").join("..").join("fav.jpg"), &canon));
        assert!(!is_inside(&dir.join("sub").join("other.jpg"), &canon));
        assert!(!is_inside(&dir.join("missing.jpg"), &canon));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
