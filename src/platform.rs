//! OS-specific parts: setting the wallpaper and registering the periodic task.
//! Windows is implemented; Linux and macOS are stubs awaiting backends.

use anyhow::Result;
use std::path::Path;

#[cfg(windows)]
pub fn set_wallpaper(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            wide.as_ptr() as *mut _,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };
    if ok == 0 {
        anyhow::bail!("SystemParametersInfoW failed");
    }
    Ok(())
}

#[cfg(windows)]
pub fn install_schedule(hours: u32) -> Result<()> {
    let exe = std::env::current_exe()?;
    // `conhost --headless` runs the console program without showing a window.
    let tr = format!("conhost.exe --headless \"{}\" run", exe.display());
    let status = std::process::Command::new("schtasks")
        .args(["/Create", "/F", "/TN", "NoBloatWallpaper", "/SC", "HOURLY", "/MO"])
        .arg(hours.to_string())
        .args(["/TR", &tr])
        .status()?;
    if !status.success() {
        anyhow::bail!("schtasks /Create failed");
    }
    Ok(())
}

#[cfg(windows)]
pub fn remove_schedule() -> Result<()> {
    let status = std::process::Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", "NoBloatWallpaper"])
        .status()?;
    if !status.success() {
        anyhow::bail!("schtasks /Delete failed (was it installed?)");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_wallpaper(_path: &Path) -> Result<()> {
    anyhow::bail!("setting the wallpaper is not implemented on this OS yet (contributions welcome)")
}
#[cfg(not(windows))]
pub fn install_schedule(_hours: u32) -> Result<()> {
    anyhow::bail!("scheduler registration (cron/launchd/systemd timer) is not implemented on this OS yet")
}
#[cfg(not(windows))]
pub fn remove_schedule() -> Result<()> {
    anyhow::bail!("scheduler registration is not implemented on this OS yet")
}
