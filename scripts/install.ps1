# One-line installer for EasyQuickScreenshot.
# Downloads the latest released eqs.exe (+ optional eqs-settings.exe) into the user's
# LOCALAPPDATA and launches it. No admin rights, no installer, nothing global.
# Run it with:  irm https://raw.githubusercontent.com/david-vrba/EasyQuickScreenshot/main/scripts/install.ps1 | iex

[CmdletBinding()]
param(
    # Also register EasyQuickScreenshot to start when you log in.
    [switch]$Autostart,
    # Where to install (defaults to a per-user folder — no admin needed).
    [string]$Dir = (Join-Path $env:LOCALAPPDATA 'EasyQuickScreenshot')
)

$ErrorActionPreference = 'Stop'
$repo = 'david-vrba/EasyQuickScreenshot'

Write-Host "Installing EasyQuickScreenshot..." -ForegroundColor Cyan

# 1. Find the latest release and its downloadable assets.
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" `
        -Headers @{ 'User-Agent' = 'eqs-install' }
} catch {
    Write-Host "Couldn't reach the GitHub API. Check your connection and try again." -ForegroundColor Red
    return
}

$core = $release.assets | Where-Object name -eq 'eqs.exe' | Select-Object -First 1
if (-not $core) {
    Write-Host "This release has no eqs.exe asset yet. Grab it from:" -ForegroundColor Red
    Write-Host "  https://github.com/$repo/releases/latest"
    return
}
$ui = $release.assets | Where-Object name -eq 'eqs-settings.exe' | Select-Object -First 1

# 2. Download into the target folder.
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$exePath = Join-Path $Dir 'eqs.exe'
Write-Host "  downloading eqs.exe ($([math]::Round($core.size/1KB)) KB) -> $Dir"
Invoke-WebRequest $core.browser_download_url -OutFile $exePath

if ($ui) {
    Write-Host "  downloading eqs-settings.exe (optional UI)"
    Invoke-WebRequest $ui.browser_download_url -OutFile (Join-Path $Dir 'eqs-settings.exe')
}

# 3. Optional: start on login (opt-in — no silent registry changes).
if ($Autostart) {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
        -Name 'EasyQuickScreenshot' -Value "`"$exePath`""
    Write-Host "  registered to start when you log in."
}

# 4. Launch it.
Start-Process $exePath

Write-Host ""
Write-Host "Done — EasyQuickScreenshot $($release.tag_name) is running in your tray." -ForegroundColor Green
Write-Host "  Ctrl+Alt+Q  quick shot (overwrites shots/temp.png)"
Write-Host "  Ctrl+Alt+E  save a timestamped shot"
Write-Host "  Ctrl+Shift+Alt+E  open the save folder"
if (-not $Autostart) {
    Write-Host ""
    Write-Host "Want it every time you log in? Re-run with:  ... | iex -Autostart" -ForegroundColor DarkGray
    Write-Host "  or:  & { irm https://raw.githubusercontent.com/$repo/main/scripts/install.ps1 } -Autostart"
}
