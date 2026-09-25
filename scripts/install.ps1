# Builds jot in release mode and installs it for the current user, with a Start Menu shortcut.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dest = Join-Path $env:LOCALAPPDATA 'Programs\jot'

Push-Location $root
try {
    if (Get-Command mise -ErrorAction SilentlyContinue) { mise x -- cargo build --release } else { cargo build --release }
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally { Pop-Location }

New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item (Join-Path $root 'target\release\jot.exe') $dest -Force

$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\jot.lnk'
$shell = New-Object -ComObject WScript.Shell
$sc = $shell.CreateShortcut($lnk)
$sc.TargetPath = Join-Path $dest 'jot.exe'
$sc.WorkingDirectory = $dest
$sc.IconLocation = "$(Join-Path $dest 'jot.exe'),0"
$sc.Save()

Write-Host "Installed to $dest"
Write-Host "Start Menu shortcut: $lnk"
