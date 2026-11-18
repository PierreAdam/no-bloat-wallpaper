# no-bloat-wallpaper

A tiny wallpaper rotator that **does not stay running**.

It is a ~2 MB command-line program (`nbw`) that your operating system's own
scheduler launches every few hours. It fetches an image, sets it as your
wallpaper, and exits. Between runs it uses **0 MB of RAM**: no tray icon, no
desktop widget, no background service.

## Why

Daily-wallpaper apps tend to grow: a permanent process holding tens of MB of
RAM, an icon you cannot remove, a single hard-wired image source. This project
is the opposite:

- **Nothing resident.** The OS scheduler (Task Scheduler / cron / launchd) is the
  only thing that "runs" all day.
- **Multiple sources**, picked from built-in ones or defined by you.
- **Transparent licensing.** Every image is shown with its credit and license
  info when the source provides it.
- **Open source** (MIT), small enough to read in an afternoon.

## Status

| Feature | Windows | Linux | macOS |
|---|---|---|---|
| Fetch + set wallpaper | yes | not yet | not yet |
| Scheduler registration | yes (Task Scheduler) | not yet | not yet |

Planned: Linux/macOS backends, an on-demand GUI (Tauri, opened only when you
want to edit sources or browse favorites, then closed), packaged installers.

## Install (from source, for now)

Requires [Rust](https://rustup.rs).

```powershell
cargo build --release
.\target\release\nbw.exe install     # registers the periodic task
.\target\release\nbw.exe run         # or apply a wallpaper right now
```

## Commands

| Command | What it does |
|---|---|
| `nbw run` | Fetch a new image and set it (what the scheduler calls) |
| `nbw install` | Register the periodic task using `interval_hours` |
| `nbw uninstall` | Remove the periodic task |
| `nbw favorite` | Copy the current wallpaper and its license info to the favorites folder |
| `nbw config` | Print the config path (creates a default one) |

## Configuration

`nbw config` prints the path of `config.toml`. Example:

```toml
interval_hours = 6        # re-run `nbw install` after changing this
rotation = "random"       # or "latest"

[[sources]]
id = "bing"
kind = "bing"
market = "en-US"         # e.g. fr-FR, de-DE, ja-JP

[[sources]]
id = "nasa"
kind = "nasa-apod"
api_key = "DEMO_KEY"      # get a free key at https://api.nasa.gov

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
license = "CC BY 4.0"
enabled = false
```

Built-in kinds: `bing`, `nasa-apod`, `folder`, `json`. If one source fails
(offline, rate limit), it is skipped and the others still work.

## Disclaimer: images and licenses

**This project does not host, store, mirror or distribute any image.** It only
downloads images, on your computer, from public third-party services you
configure, exactly as your web browser would.

Images remain the property of their respective authors and are subject to
their own licenses and terms of use. **You are solely responsible for checking
the license of an image before keeping, copying, sharing or reusing it.**
`nbw` displays the credit and license information provided by each source when
available, but cannot guarantee it is complete or accurate.

Some sources (for example Bing's daily image endpoint) are unofficial and may
change or stop working at any time. This project is not affiliated with or
endorsed by Microsoft, NASA, or any other image provider.

## Contributing

Issues and pull requests welcome, especially Linux/macOS backends (see
`src/platform.rs`) and new source adapters (see `src/sources.rs`).

## License

Code: [MIT](LICENSE). Images: see disclaimer above.
