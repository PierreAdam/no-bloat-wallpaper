# no-bloat-wallpaper

A tiny wallpaper rotator that **does not stay running**.

Your operating system's own scheduler launches the app (`nbw-gui --task`) every few
hours. It fetches an image, sets it as your wallpaper, and exits, without opening a
window. The same program is the GUI you use to set things up; a command-line tool
(`nbw`) is there too if you prefer it. Between runs it uses **0 MB of RAM**: no
desktop widget, no background service. If you want one-click changes, you can turn on an
[optional tray icon](#tray-icon-optional) (about 4 MB).

## Why

Daily-wallpaper apps tend to grow: a permanent process holding tens of MB of
RAM, an icon you cannot remove, a single hard-wired image source. This project
is the opposite:

- **Nothing resident by default.** The OS scheduler (Task Scheduler / cron / launchd) is the
  only thing that "runs" all day. The tray icon is opt-in, and the app tells you what it costs.
- **Multiple sources**, picked from built-in ones or defined by you.
- **Transparent licensing.** Every image is shown with its credit and license
  info when the source provides it.
- **Open source** (MIT), small enough to read in an afternoon.

## Status

| Feature | Windows | Linux | macOS |
|---|---|---|---|
| Fetch + set wallpaper | yes | not yet | not yet |
| Scheduler registration | yes (Task Scheduler) | not yet | not yet |

The app (`nbw-gui`, built with Tauri) lets you edit sources, set the schedule, trigger a
refresh and browse your favorites with their licenses. Its window is only open while you
use it; close it and nothing stays in memory (unless you turned on the tray icon). Windows
has an installer and a portable zip.

Planned: Linux/macOS backends.

## GUI

```powershell
.\build.ps1 -Run        # Windows (use ./build.sh --run on Linux, macOS, Git Bash)
```

`nbw` and `nbw-gui` are both built into `target/release`. The scheduled tasks run
`nbw-gui.exe --task` (periodic) and `nbw-gui.exe --task --if-due` (after login): in that
mode it changes the wallpaper and exits before any window or web view is created. Keep
`nbw.exe` next to `nbw-gui.exe` if you use the CLI: `nbw install` registers the
`nbw-gui.exe` beside it. Each scheduled run writes its outcome to `last-run.log` in the
data folder.

On Windows it uses the WebView2 runtime that ships with Windows 10/11.

The settings have two tabs: **System** (schedule, tray icon, language, cache) and
**Sources** (where pictures come from). The scheduler card says plainly whether the
schedule is **enabled** or **not enabled**: until you turn it on, nothing changes on its
own.

## Tray icon (optional)

Off by default. Turn it on in **Settings > System** to get an icon next to the clock:

- **Left-click**: a small panel opens above the icon with the current picture, "Change now",
  your sources' previews (click one to use it) and a button to open the full window.
  Click elsewhere or press Esc to close it.
- **Right-click**: open the window, or quit.

What it costs, as shown in Task Manager (WebView2's processes included):

| | Memory |
|---|---|
| Tray icon only | about 2-4 MB |
| Panel or window open | about 76 MB, given back as soon as it closes |

The panel and the window are destroyed when they close, not hidden, so the web view does
not linger. The scheduled tasks still do the rotation: quitting the tray icon never stops
it. Turning the icon on adds a login startup entry for your user
(`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `NoBloatWallpaperTray`);
turning it off, or uninstalling, removes it.

## Install (Windows)

Download from the Releases page:

- **`No Bloat Wallpaper_x.y.z_x64-setup.exe`**: installer with a Start menu shortcut.
  It installs for your user only (no admin rights), fetches the WebView2 runtime if
  your PC lacks it, and removes both scheduled tasks (and the tray icon's login entry)
  when you uninstall. Upgrading keeps them. Tick "Delete the application data" when
  uninstalling to also remove your settings, downloads and favorites
  (`%APPDATA%\no-bloat-wallpaper`).
- **`no-bloat-wallpaper-windows-x64.zip`**: portable. Unzip anywhere and keep
  `nbw-gui.exe` (the app, which the scheduler also runs) and `nbw.exe` (the optional
  command line) in the same folder. If you move the folder later, turn the schedule
  off and on again in the app.

The programs are not code-signed, so Windows SmartScreen may warn on first launch
("More info", then "Run anyway").

## Build from source

Requires [Rust](https://rustup.rs), and [Node.js](https://nodejs.org) only to make the
installer.

```powershell
.\build.ps1                          # builds nbw.exe and nbw-gui.exe into target\release
.\build.ps1 -Package                 # also makes the installer and the portable zip
.\target\release\nbw.exe install     # registers the scheduled tasks (they run nbw-gui.exe)
.\target\release\nbw.exe run         # or apply a wallpaper right now
```

`.\build.ps1 -Help` lists every option; `./build.sh` does the same from Git Bash, Linux
or macOS. On Windows the scheduled tasks run `nbw-gui.exe`, so build the GUI too (not only
`cargo build`) before `nbw install`.

## Surviving restarts

Scheduled tasks live in the OS, not in a running program, so the schedule is still there
after a restart and the app never needs to be open. The interval can be anything from 1 to
168 hours (one week). If the PC was off at the scheduled time, Windows starts the missed run
as soon as it can, and **Start with Windows** (on by default, `check_on_login` in the config) adds a second
task that runs shortly after you log in (`login_delay_secs`, default 60). It refreshes the
wallpaper only if there is something to catch up on (in `latest` mode: the first source
has a newer image than the one showing; in `random` mode: the last change is older than
your interval), otherwise it does nothing. If the network is not up yet, it retries a few
times.

On a laptop, **Run on battery** (on by default, `run_on_battery` in the config) decides
whether the tasks may also run when the PC is unplugged. The GUI shows this option only on
computers with a battery. A run never lasts more than 10 minutes, and runs never overlap:
every download has a time limit.

Changing the interval, the login check or the battery option in the GUI updates the
registered tasks right away when the schedule is on. From the command line, run
`nbw install` again after editing the config.

### Latest mode and your own picks

With `rotation = "latest"` (the default) the wallpaper is the **newest image of the first
enabled source**. A scheduled run only changes it when that newest image is different from
what is showing, so it does not wander back through older pictures. If you pick another
image by hand (from the previews, or a favorite), your pick **stays until the source
publishes a newer image**, or until you press "Change now". In `latest` mode "Change now"
asks the source again and loads its newest image right away (even over your pick), and
simply says so if that image is already showing. In `random` mode it picks a random image
you have not seen yet; once you have seen them all, the history starts over (without
repeating the picture showing now). Every scheduled run adds a line with its time and
outcome to `last-run.log` in the data folder (the last 200 runs are kept).

To try it without rebooting, run `scripts\test-login-check.ps1` in a normal terminal.

## Cache: being polite to the sources

What the online sources offer is saved on your computer and reused for `cache_minutes`
(default **20**), so opening the app or running several scheduled checks does not hit
their servers again and again. Previews are made once, as small files on disk (a large
picture from a local folder or a custom endpoint is shrunk), and a full-size picture that
was already downloaded is reused. If a source is
unreachable or rate-limits you, the last saved answer is used instead of failing.
Downloaded files older than 30 days are deleted (the wallpaper in use is always kept).
The **Cache** card in the settings shows the space used and can empty it; the refresh
button on the main screen asks the sources again right away.

## Where things are stored

On Windows, under `%APPDATA%\no-bloat-wallpaper\` (`nbw config` prints the exact path of
the config file; Linux and macOS use the platform's standard config and data folders):

| What | Where |
|---|---|
| Settings | `config\config.toml` |
| Current image, history, last change time | `data\state.json` |
| Cached answers and previews | `data\cache\` |
| Full-size downloaded pictures | `data\images\` |
| Favorites (each with its license info) | `data\favorites\` |
| Time and outcome of recent scheduled runs | `data\last-run.log` |
| Lock that keeps the app and the scheduled task from writing at the same time | `data\nbw.lock` |

## Languages

The interface is available in English, Français, Deutsch, Español, Italiano, Čeština,
Português (Brasil), Nederlands and 日本語.
**Automatic** (default) follows your system language, or pick one in the settings
(`language` in the config). Translations are plain files in `gui/ui/lang/`: to add or
fix a language, copy `en.js`, translate the values (keep the `{placeholders}`), register
it in `gui/ui/i18n.js` and `index.html`, then run `node scripts/check-translations.js`.
The command-line program and the log messages are English only for now.

## Commands

| Command | What it does |
|---|---|
| `nbw run` | Change the wallpaper now (like "Change now" in the GUI) |
| `nbw run --if-due` | Change it only if there is something to catch up on (like the login check) |
| `nbw install` | Register the periodic task, plus the login check if `check_on_login` is on |
| `nbw uninstall` | Remove both scheduled tasks |
| `nbw favorite` | Copy the current wallpaper and its license info to the favorites folder |
| `nbw config` | Print the config path (creates a default one) |
| `nbw check` | Test every enabled source: updates the source cache, the wallpaper does not change |

## Configuration

`nbw config` prints the path of `config.toml`. Example:

```toml
interval_hours = 6        # 1 to 168; re-run `nbw install` after changing this
rotation = "latest"       # default: newest image of the first enabled source; or "random"
                          # the order of the [[sources]] below matters (the GUI has arrows to reorder)
check_on_login = true     # catch up shortly after you log in
login_delay_secs = 60
run_on_battery = true     # laptops: also run when unplugged
cache_minutes = 20        # reuse the sources' answers this long (0 = no cache)
language = "auto"         # interface language: "auto" or a code such as "fr"
tray = false              # GUI only: optional tray icon (starts at login while on)

[[sources]]
id = "bing"
kind = "bing"
market = "en-US"         # e.g. fr-FR, de-DE, ja-JP

[[sources]]
id = "nasa"
kind = "nasa-apod"

[[sources]]
id = "wikimedia"
kind = "wikimedia-potd"

[[sources]]
id = "webb"
kind = "esa-webb"

[[sources]]
id = "mine"
kind = "folder"
path = "C:\\Users\\me\\Pictures\\Wallpapers"

[[sources]]
id = "custom"
kind = "json"             # your own endpoint returning a JSON array
url = "https://example.com/images.json"
image_field = "url"
title_field = "title"
thumb_field = "thumb"     # optional: a small preview URL in each item
license = "CC BY 4.0"
enabled = false
```

Sources start **disabled**: a fresh install asks you to enable at least one (in the GUI
settings, or with `enabled = true` here). `nasa-apod` reads NASA's official picture-of-the-day
RSS feed (<https://science.nasa.gov/feed/apod-basic/>): no API key, no rate limit, the last
30 days, videos skipped.
`wikimedia-potd` uses the Wikimedia Commons API for the last 14 pictures of the day, each
with its own author and license, downloaded as 4K scaled copies (never the originals).
`esa-webb` reads ESA/Webb's Picture of the Month RSS feed
(<https://esawebb.org/images/potm/feed/>): the last 25 James Webb pictures, CC BY 4.0 with a
mandatory full credit line (see each image's page on esawebb.org).

Kinds: `bing`, `nasa-apod`, `wikimedia-potd`, `esa-webb`, `folder`, `json`. The first four
are built in and can appear only once each (remove one and you can add it back later); you can have as many folders
and JSON endpoints as you like. Every source needs a unique `id`. If one source fails
(offline, rate limit), it is skipped and the others still work.

## Disclaimer: images and licenses

(Also shipped as [DISCLAIMER.md](DISCLAIMER.md) in every package.)

**This project does not host, store, mirror or distribute any image.** It only
downloads images, on your computer, from public third-party services you
configure, exactly as your web browser would.

Images remain the property of their respective authors and are subject to
their own licenses and terms of use. **You are solely responsible for checking
the license of an image before keeping, copying, sharing or reusing it.**
The app displays the credit and license information provided by each source when
available, but cannot guarantee it is complete or accurate.

Some sources (for example Bing's daily image endpoint) are unofficial and may
change or stop working at any time. This project is not affiliated with or
endorsed by Microsoft, NASA, the Wikimedia Foundation, ESA, or any other image
provider.

## Contributing

Issues and pull requests welcome, especially Linux/macOS backends (see
`src/platform.rs`) and new source adapters (see `src/sources.rs`).

## License

Code: [MIT](LICENSE). Images: see disclaimer above.
