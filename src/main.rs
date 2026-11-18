mod config;
mod platform;
mod sources;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sources::{Candidate, Location};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Tiny scheduler-driven wallpaper rotator")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch a new wallpaper and apply it (this is what the scheduler runs).
    Run,
    /// Register the periodic task with the OS scheduler, using `interval_hours`.
    Install,
    /// Remove the periodic task.
    Uninstall,
    /// Copy the current wallpaper (and its license info) into the favorites folder.
    Favorite,
    /// Print the config file path, creating a default one if needed.
    Config,
    /// Query every enabled source and report how many images each offers (changes nothing).
    Check,
}

/// What we remember between runs.
#[derive(Default, Serialize, Deserialize)]
struct State {
    seen: Vec<String>,
    current: Option<Current>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Current {
    id: String,
    title: String,
    credit: Option<String>,
    license: String,
    file: PathBuf,
}

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
    let p = state_path()?;
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Run => run(),
        Cmd::Install => {
            let cfg = config::load()?;
            platform::install_schedule(cfg.interval_hours)?;
            println!("Scheduled every {}h.", cfg.interval_hours);
            Ok(())
        }
        Cmd::Uninstall => platform::remove_schedule(),
        Cmd::Favorite => favorite(),
        Cmd::Check => {
            let cfg = config::load()?;
            for src in cfg.sources.iter().filter(|s| s.enabled) {
                match sources::fetch(src) {
                    Ok(c) => println!("[{}] ok: {} image(s), e.g. \"{}\"", src.id, c.len(), c.first().map(|x| x.title.as_str()).unwrap_or("-")),
                    Err(e) => println!("[{}] FAILED: {e:#}", src.id),
                }
            }
            Ok(())
        }
        Cmd::Config => {
            config::load()?;
            println!("{}", config::config_path()?.display());
            Ok(())
        }
    }
}

fn run() -> Result<()> {
    let cfg = config::load()?;
    let mut state = load_state();

    let mut all: Vec<Candidate> = Vec::new();
    for src in cfg.sources.iter().filter(|s| s.enabled) {
        match sources::fetch(src) {
            Ok(c) => all.extend(c),
            Err(e) => eprintln!("[{}] skipped: {e:#}", src.id),
        }
    }
    all.retain(|c| !state.seen.contains(&c.id));
    if all.is_empty() {
        // Everything has been seen: start over rather than doing nothing.
        state.seen.clear();
        save_state(&state)?;
        anyhow::bail!("no new images available this time; history reset, will retry next run");
    }

    // Try candidates until one downloads as a valid image; remember bad ones so they are skipped.
    let (pick, file) = loop {
        if all.is_empty() {
            save_state(&state)?;
            anyhow::bail!("no usable image found this time");
        }
        let pick = if cfg.rotation == "latest" {
            all.remove(0)
        } else {
            all.swap_remove(fastrand::usize(..all.len()))
        };
        match materialize(&pick) {
            Ok(file) => break (pick, file),
            Err(e) => {
                eprintln!("[{}] rejected: {e:#}", pick.id);
                state.seen.push(pick.id);
            }
        }
    };
    platform::set_wallpaper(&file)?;

    state.seen.push(pick.id.clone());
    if state.seen.len() > 500 {
        state.seen.drain(..100);
    }
    state.current = Some(Current {
        id: pick.id,
        title: pick.title.clone(),
        credit: pick.credit.clone(),
        license: pick.license.clone(),
        file,
    });
    save_state(&state)?;

    println!("Set: {}", pick.title);
    if let Some(c) = &pick.credit {
        println!("Credit: {c}");
    }
    println!("License: {}", pick.license);
    Ok(())
}

/// Download (or reference) the chosen image and return a local file path.
fn materialize(c: &Candidate) -> Result<PathBuf> {
    match &c.location {
        Location::File(p) => Ok(p.clone()),
        Location::Url(url) => {
            let dir = config::data_dir()?.join("images");
            std::fs::create_dir_all(&dir)?;
            let safe: String = c
                .id
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '_' })
                .collect();
            let file = dir.join(format!("{safe}.jpg"));
            let resp = ureq::get(url).set("User-Agent", "no-bloat-wallpaper").call()?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes)?;
            let is_jpeg = bytes.starts_with(&[0xFF, 0xD8]);
            let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
            if !(is_jpeg || is_png) {
                anyhow::bail!("not a JPEG/PNG image");
            }
            if bytes.len() < 50_000 {
                anyhow::bail!("image too small ({} bytes), likely a placeholder", bytes.len());
            }
            std::fs::write(&file, bytes)?;
            Ok(file)
        }
    }
}

fn favorite() -> Result<()> {
    let state = load_state();
    let cur = state.current.context("no wallpaper has been set yet")?;
    let dir = config::data_dir()?.join("favorites");
    std::fs::create_dir_all(&dir)?;
    let name = cur.file.file_name().context("bad file name")?;
    std::fs::copy(&cur.file, dir.join(name))?;
    let info = dir.join(format!("{}.json", cur.file.file_stem().unwrap().to_string_lossy()));
    std::fs::write(info, serde_json::to_string_pretty(&cur)?)?;
    println!("Saved to {}", dir.display());
    println!("License: {}", cur.license);
    println!("Reminder: you are responsible for checking the license before keeping or reusing an image.");
    Ok(())
}
