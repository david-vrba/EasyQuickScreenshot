# Registers (or removes) EasyQuickScreenshot in the current user's autostart.
# Usage: pwsh scripts/autostart.ps1 [-Remove]

param([switch]$Remove)

$keyPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$name = "EasyQuickScreenshot"

if ($Remove) {
    try { Remove-ItemProperty -Path $keyPath -Name $name -ErrorAction Stop } catch {}
    Write-Output "Autostart removed."
    exit 0
}

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "eqs.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $root "target\release\eqs.exe" }
if (-not (Test-Path $exe)) {
    Write-Error "eqs.exe not found. Build it first: cargo build --release"
    exit 1
}

Set-ItemProperty -Path $keyPath -Name $name -Value "`"$exe`""
Write-Output "Autostart registered: $exe"
