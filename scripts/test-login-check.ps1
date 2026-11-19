<#
.SYNOPSIS
  Tests the "Start with Windows" check without rebooting.
.DESCRIPTION
  Runs the login task twice via Task Scheduler and prints what it decided:
    1) pretends the last change was 1 hour ago  -> expect "skipped"
    2) pretends the last change was 7 hours ago -> expect "ok: <new wallpaper>" (with a 6 h interval)
  This test is about the "random" rotation, where "overdue" means the last change is older than
  the interval. With rotation = "latest" (the default) the login check instead asks whether the
  first source has a newer image, so expect "skipped: already showing the newest image" in both
  steps unless a newer image really exists.
  Run it from a normal terminal. It edits state.json in your data folder; the app rewrites it
  on the next real change, so nothing needs restoring.
#>
$ErrorActionPreference = "Stop"
$dir = Join-Path $env:APPDATA "no-bloat-wallpaper\data"
$state = Join-Path $dir "state.json"
$log = Join-Path $dir "last-run.log"

# Native stderr must not become a terminating error in Windows PowerShell 5.1.
$ErrorActionPreference = "Continue"
schtasks /Query /TN NoBloatWallpaper-Login *> $null
$registered = ($LASTEXITCODE -eq 0)
$ErrorActionPreference = "Stop"
if (-not $registered) {
    throw "The login task is not registered. Turn the schedule on first (app: Settings > System > Save and turn on, or: nbw install)."
}

function Set-LastChange([double]$hoursAgo) {
    $j = Get-Content $state -Raw | ConvertFrom-Json
    $j.last_run = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - [long]($hoursAgo * 3600)
    # UTF-8 without BOM (Windows PowerShell 5.1's Set-Content would add one).
    [IO.File]::WriteAllText($state, ($j | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))
}

function Invoke-LoginTask {
    Remove-Item $log -ErrorAction SilentlyContinue
    schtasks /Run /TN NoBloatWallpaper-Login | Out-Null
    for ($i = 0; $i -lt 90 -and -not (Test-Path $log); $i++) { Start-Sleep 1 }
    Start-Sleep 1
    if (Test-Path $log) { Get-Content $log } else { "no log written (task did not finish in 90 s)" }
}

if (-not (Test-Path $state)) { throw "No state.json yet. Change the wallpaper once first (nbw run)." }

Set-LastChange 1
"1) last change 1 h ago  -> " + (Invoke-LoginTask)
Set-LastChange 7
"2) last change 7 h ago  -> " + (Invoke-LoginTask)
