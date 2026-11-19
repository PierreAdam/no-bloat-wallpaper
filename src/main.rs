use anyhow::Result;
use clap::{Parser, Subcommand};
use no_bloat_wallpaper::{config, engine, platform};

#[derive(Parser)]
#[command(version, about = "Tiny scheduler-driven wallpaper rotator")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch a new wallpaper and apply it now (like "Change now" in the app).
    Run {
        /// Behave like the after-login check: skip if there is nothing new (latest mode, a manual
        /// pick is respected) or nothing overdue (random mode).
        #[arg(long)]
        if_due: bool,
    },
    /// Register the periodic task (and the after-login check, if enabled) with the OS scheduler.
    Install,
    /// Remove the scheduled tasks (periodic and after-login).
    Uninstall,
    /// Copy the current wallpaper (and its license info) into the favorites folder.
    Favorite,
    /// Print the config file path, creating a default one if needed.
    Config,
    /// Query every enabled source and report how many images each offers. The answers are saved
    /// in the source cache; the wallpaper does not change.
    Check,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Run { if_due } => {
            let outcome = if if_due { engine::run_if_due()? } else { engine::refresh_now()? };
            let cur = match outcome {
                engine::Outcome::Changed(c) => c,
                engine::Outcome::Skipped(why) => {
                    println!("Nothing to do: {why}.");
                    return Ok(());
                }
            };
            println!("Set: {}", cur.title);
            if let Some(c) = &cur.credit {
                println!("Credit: {c}");
            }
            println!("License: {}", cur.license);
            Ok(())
        }
        Cmd::Install => {
            let hours = engine::apply_schedule()?;
            let cfg = config::load()?;
            println!("Scheduled every {hours}h.");
            if cfg.check_on_login {
                println!("Also checking {}s after login.", cfg.login_delay_secs);
            }
            Ok(())
        }
        Cmd::Uninstall => platform::remove_schedule(),
        Cmd::Favorite => {
            let fav = engine::favorite_current()?;
            println!("Saved to {}", fav.file.display());
            println!("License: {}", fav.license);
            println!("Reminder: you are responsible for checking the license before keeping or reusing an image.");
            Ok(())
        }
        Cmd::Check => {
            for (id, res) in engine::check_sources()? {
                match res {
                    Ok(c) => println!(
                        "[{id}] ok: {} image(s), e.g. \"{}\"",
                        c.len(),
                        c.first().map(|x| x.title.as_str()).unwrap_or("-")
                    ),
                    Err(e) => println!("[{id}] FAILED: {e:#}"),
                }
            }
            println!("The source cache was updated; the wallpaper did not change.");
            Ok(())
        }
        Cmd::Config => {
            config::load()?;
            println!("{}", config::config_path()?.display());
            Ok(())
        }
    }
}
