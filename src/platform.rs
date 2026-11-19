//! OS-specific parts: setting the wallpaper and registering the periodic task.
//! Windows is implemented; Linux and macOS are stubs awaiting backends.

use anyhow::Result;
use serde::Serialize;
use std::path::Path;

/// What the OS scheduler actually has registered for us.
#[derive(Serialize, Clone)]
pub struct ScheduleInfo {
    /// Local time the repetition started, as written by the OS ("2026-11-18T18:26:00").
    /// Interpret without a timezone (= local time).
    pub start: String,
    /// Repeat interval in hours (0 if it could not be determined).
    pub interval_hours: u32,
}

/// How the scheduled tasks are registered.
pub struct TaskOptions {
    pub interval_hours: u32,
    /// Let the tasks start, and keep running, on battery power.
    pub run_on_battery: bool,
}

/// Hours in an ISO 8601 duration such as "PT6H", "P1D" or "P1DT12H" (minutes and seconds
/// are ignored; 0 if it cannot be read).
pub fn iso_duration_hours(iv: &str) -> u32 {
    let Some(rest) = iv.strip_prefix('P') else { return 0 };
    let mut hours = 0;
    let mut num = String::new();
    let mut in_time = false;
    for ch in rest.chars() {
        match ch {
            'T' => in_time = true,
            c if c.is_ascii_digit() => num.push(c),
            unit => {
                let n: u32 = num.parse().unwrap_or(0);
                num.clear();
                match (unit, in_time) {
                    ('W', false) => hours += n * 168,
                    ('D', false) => hours += n * 24,
                    ('H', true) => hours += n,
                    _ => {}
                }
            }
        }
    }
    hours
}

#[cfg(windows)]
const TASK: &str = "NoBloatWallpaper";
#[cfg(windows)]
const LOGIN_TASK: &str = "NoBloatWallpaper-Login";

#[cfg(windows)]
pub fn set_wallpaper(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, wide.as_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE)
    };
    if ok == 0 {
        anyhow::bail!("SystemParametersInfoW failed");
    }
    Ok(())
}

/// Name of the value under the user's `Run` key that starts the tray icon at login.
#[cfg(windows)]
const RUN_VALUE: &str = "NoBloatWallpaperTray";
#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Start `command` when you log in (per user, no admin rights), or stop doing so with `None`.
#[cfg(windows)]
pub fn set_login_autostart(command: Option<&str>) -> Result<()> {
    use anyhow::Context;
    use std::os::windows::process::CommandExt;
    let mut reg = std::process::Command::new("reg");
    match command {
        Some(cmd) => reg.args(["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", cmd, "/f"]),
        None => {
            if !login_autostart_set() {
                return Ok(());
            }
            reg.args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
        }
    };
    // `.output()` for the same reason as in `schtasks()` below: the GUI has no console.
    let out = reg.creation_flags(0x0800_0000).output().context("could not run reg")?;
    if !out.status.success() {
        anyhow::bail!("could not update the login startup entry: {}", decode(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(windows)]
fn login_autostart_set() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(["query", RUN_KEY, "/v", RUN_VALUE])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Two-letter code of the Windows display language ("fr"), for text shown outside the page.
#[cfg(windows)]
pub fn ui_language() -> &'static str {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    match unsafe { GetUserDefaultUILanguage() } & 0x3ff {
        0x0c => "fr",
        0x07 => "de",
        0x0a => "es",
        0x10 => "it",
        0x05 => "cs",
        0x16 => "pt",
        0x13 => "nl",
        0x11 => "ja",
        _ => "en",
    }
}

/// Whether this computer has a battery (a laptop or tablet), so the UI can offer the
/// "run on battery" option only where it means something.
#[cfg(windows)]
pub fn has_battery() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut st: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut st) } == 0 {
        return false;
    }
    // 128 = no system battery, 255 = unknown status.
    st.BatteryFlag != 128 && st.BatteryFlag != 255
}

/// Locate `nbw-gui.exe`, which the scheduler runs with `--task` (it has no console window, so
/// nothing flashes on screen): a sibling of the current executable (installed layout). Debug
/// builds (tests, `cargo run`) also look for a release build in an ancestor's `target/release`;
/// release builds never do, so they cannot register some other copy found up the tree.
#[cfg(windows)]
fn task_exe() -> Result<std::path::PathBuf> {
    let me = std::env::current_exe()?;
    let sibling = me.with_file_name("nbw-gui.exe");
    if sibling.exists() {
        return Ok(sibling);
    }
    #[cfg(debug_assertions)]
    for dir in me.ancestors().skip(1) {
        let dev = dir.join("target").join("release").join("nbw-gui.exe");
        if dev.exists() {
            return Ok(dev);
        }
    }
    anyhow::bail!("could not find nbw-gui.exe next to this program (the scheduler runs it to change the wallpaper)")
}

/// Run `schtasks` and capture everything. Always use `.output()`: the GUI has no console,
/// and a child that inherits its invalid stdio handles fails with
/// "The request is not supported. (os error 50)". `.output()` pipes all three streams.
#[cfg(windows)]
fn schtasks(args: &[&str]) -> Result<std::process::Output> {
    use anyhow::Context;
    use std::os::windows::process::CommandExt;
    std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .context("could not run schtasks")
}

/// schtasks writes either UTF-16 or the OEM code page depending on how it is redirected.
#[cfg(windows)]
fn decode(bytes: &[u8]) -> String {
    if bytes.len() > 1 && (bytes[1] == 0 || bytes.starts_with(&[0xFF, 0xFE])) {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units).trim_start_matches('\u{feff}').to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `DOMAIN\user` of the person running this program.
#[cfg(windows)]
fn current_user() -> Result<String> {
    use anyhow::Context;
    Ok(format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").context("USERNAME is not set")?
    ))
}

/// Local time now, as Task Scheduler writes it ("2026-11-18T18:26:00").
#[cfg(windows)]
pub fn local_now() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Today's date on this computer's clock, in days since 1970-01-01.
#[cfg(windows)]
pub fn local_today() -> i64 {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    days_from_civil(i64::from(t.wYear), i64::from(t.wMonth), i64::from(t.wDay))
}

/// A complete task definition. Both tasks run as you, without admin rights, and use the
/// same settings: no overlapping runs, a 10 minute limit, catch up when a start was missed,
/// and the battery choice from the settings.
#[cfg(windows)]
fn task_xml(trigger: &str, args: &str, opts: &TaskOptions) -> Result<String> {
    let user = xml_escape(&current_user()?);
    let cmd = xml_escape(&task_exe()?.display().to_string());
    let battery_stop = !opts.run_on_battery;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>{trigger}</Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>{battery_stop}</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>{battery_stop}</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT10M</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author"><Exec><Command>{cmd}</Command><Arguments>{args}</Arguments></Exec></Actions>
</Task>"#
    ))
}

/// Register (or replace) a task from its XML definition.
#[cfg(windows)]
fn register(name: &str, xml: &str) -> Result<()> {
    // schtasks wants UTF-16 LE with a byte order mark.
    let mut bytes = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("nbw-{}-{}.xml", name, std::process::id()));
    std::fs::write(&path, bytes)?;
    let path_str = path.to_string_lossy().into_owned();
    let out = schtasks(&["/Create", "/F", "/TN", name, "/XML", &path_str]);
    let _ = std::fs::remove_file(&path);
    let out = out?;
    if !out.status.success() {
        anyhow::bail!("could not register the task {name}: {}", decode(&out.stderr).trim());
    }
    Ok(())
}

/// Register the periodic task: starts now, repeats every `interval_hours`, indefinitely.
/// Defined in XML (not `/SC HOURLY`, which stops at 23 h and runs only on mains power).
#[cfg(windows)]
pub fn install_schedule(opts: &TaskOptions) -> Result<()> {
    let trigger = format!(
        "<TimeTrigger><Enabled>true</Enabled><StartBoundary>{start}</StartBoundary>\
         <Repetition><Interval>PT{h}H</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition></TimeTrigger>",
        start = local_now(),
        h = opts.interval_hours.max(1),
    );
    register(TASK, &task_xml(&trigger, crate::task::TASK_ARG, opts)?)
}

/// Register a task that runs `nbw-gui --task --if-due` shortly after you log in. A log-on trigger
/// scoped to your own account needs no admin rights (unlike `schtasks /SC ONLOGON`).
#[cfg(windows)]
pub fn install_login_check(delay_secs: u32, opts: &TaskOptions) -> Result<()> {
    let user = xml_escape(&current_user()?);
    let trigger = format!(
        "<LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>PT{delay_secs}S</Delay></LogonTrigger>"
    );
    let args = format!("{} {}", crate::task::TASK_ARG, crate::task::IF_DUE_ARG);
    register(LOGIN_TASK, &task_xml(&trigger, &args, opts)?)
}

/// Remove the periodic task and the after-login task. Fails only if neither existed.
#[cfg(windows)]
pub fn remove_schedule() -> Result<()> {
    let periodic = schtasks(&["/Delete", "/F", "/TN", TASK])?;
    let login = schtasks(&["/Delete", "/F", "/TN", LOGIN_TASK])?;
    if !periodic.status.success() && !login.status.success() {
        anyhow::bail!("schtasks /Delete failed: {}", decode(&periodic.stderr).trim());
    }
    Ok(())
}

#[cfg(windows)]
pub fn remove_login_check() {
    let _ = schtasks(&["/Delete", "/F", "/TN", LOGIN_TASK]);
}

#[cfg(windows)]
pub fn login_check_installed() -> bool {
    schtasks(&["/Query", "/TN", LOGIN_TASK]).map(|o| o.status.success()).unwrap_or(false)
}

/// Read the registered task (if any) so the UI can show the scheduler's real timing.
#[cfg(windows)]
pub fn schedule_info() -> Option<ScheduleInfo> {
    let out = schtasks(&["/Query", "/TN", TASK, "/XML"]).ok()?;
    if !out.status.success() {
        return None;
    }
    let xml = decode(&out.stdout);
    let tag = |name: &str| -> Option<String> {
        let open = format!("<{name}>");
        let start = xml.find(&open)? + open.len();
        let end = xml[start..].find(&format!("</{name}>"))? + start;
        Some(xml[start..end].trim().to_string())
    };
    let start = tag("StartBoundary")?;
    let interval_hours = tag("Interval").map(|iv| iso_duration_hours(&iv)).unwrap_or(0);
    Some(ScheduleInfo { start, interval_hours })
}

/// Time now as "2026-11-18T18:26:00" (UTC here: no local time zone support without Windows).
#[cfg(not(windows))]
pub fn local_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil_date((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// (year, month, day) of a number of days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_date(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Days since 1970-01-01 of a (year, month, day): the inverse of `civil_date`.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Today's date in days since 1970-01-01 (UTC here: no local time zone support without Windows).
#[cfg(not(windows))]
pub fn local_today() -> i64 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    (secs / 86_400) as i64
}
#[cfg(not(windows))]
pub fn set_login_autostart(_command: Option<&str>) -> Result<()> {
    anyhow::bail!("starting at login is not implemented on this OS yet")
}
#[cfg(not(windows))]
pub fn ui_language() -> &'static str {
    "en"
}
#[cfg(not(windows))]
pub fn has_battery() -> bool {
    false
}
#[cfg(not(windows))]
pub fn install_login_check(_delay_secs: u32, _opts: &TaskOptions) -> Result<()> {
    anyhow::bail!("the after-login check is not implemented on this OS yet")
}
#[cfg(not(windows))]
pub fn remove_login_check() {}
#[cfg(not(windows))]
pub fn login_check_installed() -> bool {
    false
}
#[cfg(not(windows))]
pub fn schedule_info() -> Option<ScheduleInfo> {
    None
}
#[cfg(not(windows))]
pub fn set_wallpaper(_path: &Path) -> Result<()> {
    anyhow::bail!("setting the wallpaper is not implemented on this OS yet (contributions welcome)")
}
#[cfg(not(windows))]
pub fn install_schedule(_opts: &TaskOptions) -> Result<()> {
    anyhow::bail!("scheduler registration (cron/launchd/systemd timer) is not implemented on this OS yet")
}
#[cfg(not(windows))]
pub fn remove_schedule() -> Result<()> {
    anyhow::bail!("scheduler registration is not implemented on this OS yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_in_hours() {
        assert_eq!(iso_duration_hours("PT6H"), 6);
        assert_eq!(iso_duration_hours("P1D"), 24);
        assert_eq!(iso_duration_hours("P1DT12H"), 36);
        assert_eq!(iso_duration_hours("P1W"), 168);
        assert_eq!(iso_duration_hours("PT30M"), 0);
        assert_eq!(iso_duration_hours("garbage"), 0);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(20_775), (2026, 11, 18));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        for days in [-1, 0, 59, 60, 11_016, 20_775, 100_000] {
            let (y, m, d) = civil_date(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    /// Registers, reads back and deletes a throwaway task with the real XML (a 36 h interval,
    /// which `/SC HOURLY` could not express). Needs a release build of nbw-gui:
    /// `cargo build --release && cargo test -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn task_xml_is_accepted_by_the_scheduler() {
        let name = "NoBloatWallpaper-SelfTest";
        let opts = TaskOptions { interval_hours: 36, run_on_battery: false };
        let trigger = format!(
            "<TimeTrigger><Enabled>true</Enabled><StartBoundary>{}</StartBoundary>\
             <Repetition><Interval>PT36H</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition></TimeTrigger>",
            local_now()
        );
        register(name, &task_xml(&trigger, crate::task::TASK_ARG, &opts).unwrap()).unwrap();
        let out = schtasks(&["/Query", "/TN", name, "/XML"]).unwrap();
        let _ = schtasks(&["/Delete", "/F", "/TN", name]);
        let xml = decode(&out.stdout);
        let interval = xml.split("<Interval>").nth(1).and_then(|r| r.split('<').next()).unwrap();
        assert_eq!(iso_duration_hours(interval), 36, "{xml}");
        assert!(xml.contains("<DisallowStartIfOnBatteries>true</DisallowStartIfOnBatteries>"), "{xml}");
    }
}
