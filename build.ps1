<#
.SYNOPSIS
  Builds the CLI (nbw) and the GUI (nbw-gui). Both end up in .\target\release
.EXAMPLE
  .\build.ps1             # build everything
  .\build.ps1 -CliOnly    # skip the GUI (much faster)
  .\build.ps1 -Run        # build, then open the GUI
  .\build.ps1 -Clean      # delete build output first
  .\build.ps1 -Package    # also make the installer and a portable zip in target\release\bundle
#>
param(
    [switch]$CliOnly,
    [switch]$Run,
    [switch]$Clean,
    [switch]$Package,
    [Alias("h")][switch]$Help
)

# Also accept the usual spellings that PowerShell doesn't bind itself (--help, /?, help).
if ($Help -or ($args | Where-Object { $_ -in "--help", "-help", "/?", "/h", "help" })) {
    @"
Builds the CLI (nbw) and the GUI (nbw-gui) into .\target\release

Usage: .\build.ps1 [options]

Options:
  -CliOnly    Build only the CLI and skip the GUI (much faster; on Windows
              the schedule needs nbw-gui, which the scheduler runs)
  -Run        Open the GUI after building
  -Clean      Delete build output first (cargo clean)
  -Package    Also make the NSIS installer and a portable zip in
              target\release\bundle (needs Node.js for npx)
  -Help, -h   Show this help (also --help, /?)

Examples:
  .\build.ps1                 # build everything
  .\build.ps1 -CliOnly        # CLI only
  .\build.ps1 -Clean -Package # clean build + installer
"@ | Write-Host
    exit 0
}
if ($args) { throw "Unknown argument(s): $($args -join ' '). Run .\build.ps1 -Help" }

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# cargo is not on PATH in terminals opened before Rust was installed.
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path $cargoBin)) {
    $env:Path += ";$cargoBin"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found. Install Rust from https://rustup.rs and reopen the terminal."
}

function Invoke-Step($label, [scriptblock]$cmd) {
    Write-Host "==> $label" -ForegroundColor Cyan
    # Cargo prints progress on stderr; don't let PowerShell treat that as a failure.
    $ErrorActionPreference = "Continue"
    & $cmd
    $ErrorActionPreference = "Stop"
    if ($LASTEXITCODE -ne 0) { throw "$label failed (exit $LASTEXITCODE)" }
}

if ($Clean) {
    # The GUI shares the same target dir, so one clean covers both.
    Invoke-Step "Clean" { cargo clean }
}

Invoke-Step "Build CLI (nbw)" { cargo build --release }

if (-not $CliOnly) {
    Invoke-Step "Build GUI (nbw-gui)" { cargo build --release --manifest-path gui\src-tauri\Cargo.toml --target-dir target }
}

if ($Package) {
    if ($CliOnly) { throw "-Package needs the GUI; drop -CliOnly." }
    if (-not (Get-Command npx -ErrorAction SilentlyContinue)) {
        throw "npx not found. Install Node.js (it is only used to run the Tauri bundler)."
    }
    # Runs from gui\src-tauri so Cargo picks up its .cargo\config.toml (shared target dir).
    Push-Location gui\src-tauri
    try { Invoke-Step "Build installer (Tauri bundler)" { npx --yes "@tauri-apps/cli@2.12.6" build } }
    finally { Pop-Location }

    $bundle = Join-Path $PSScriptRoot "target\release\bundle"
    $zip = Join-Path $bundle "no-bloat-wallpaper-windows-x64.zip"
    Remove-Item $zip -ErrorAction SilentlyContinue
    $rel = Join-Path $PSScriptRoot "target\release"
    Compress-Archive -DestinationPath $zip -Path (Join-Path $rel "nbw.exe"), (Join-Path $rel "nbw-gui.exe"),
        (Join-Path $PSScriptRoot "LICENSE"), (Join-Path $PSScriptRoot "README.md"), (Join-Path $PSScriptRoot "DISCLAIMER.md")
    Write-Host ""
    Write-Host "Packages in ${bundle}:" -ForegroundColor Green
    Get-ChildItem $bundle -Recurse -File -Include *.exe, *.zip | ForEach-Object { "  {0}  ({1:N1} MB)" -f $_.FullName.Substring($bundle.Length + 1), ($_.Length / 1MB) }
}

$out = Join-Path $PSScriptRoot "target\release"
Write-Host ""
Write-Host "Done. Output in ${out}:" -ForegroundColor Green
foreach ($n in "nbw.exe", "nbw-gui.exe") {
    $f = Join-Path $out $n
    if (Test-Path $f) { "  {0}  ({1:N1} MB)" -f $n, ((Get-Item $f).Length / 1MB) }
}

if ($Run -and -not $CliOnly) {
    Start-Process (Join-Path $out "nbw-gui.exe")
}
