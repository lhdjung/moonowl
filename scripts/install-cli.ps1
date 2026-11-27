# Build the command-line tool and install it as `moonowl.exe` in
# %LOCALAPPDATA%\Programs\moonowl-cli, with pdfium.dll beside it, where the
# tool looks for it, and put that folder on the user's PATH. macOS and Linux
# have scripts/install-cli.sh.
#
# A folder of its own, because Windows does not tell `moonowl.exe` and the
# app's `Moonowl.exe` apart, so the two cannot share one. For the same
# reason, `moonowl` runs the app instead if the app's folder comes first on
# the PATH; the app's installers do not put it there.
$ErrorActionPreference = "Stop"
# "Stop" does not cover a program's exit code, so each command is checked.
function Checked {
    & $args[0] @($args | Select-Object -Skip 1)
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
Set-Location (Join-Path $PSScriptRoot "..")

& (Join-Path $PSScriptRoot "pdfium.ps1")
# Built as moonowl-cli, because it shares the target folder with the app's
# Moonowl.exe.
Checked cargo build --release -p moonowl-cli

$folder = Join-Path $env:LOCALAPPDATA "Programs\moonowl-cli"
New-Item -ItemType Directory -Force $folder | Out-Null
Copy-Item target\release\moonowl-cli.exe (Join-Path $folder "moonowl.exe") -Force
Copy-Item pdfium\bin\pdfium.dll $folder -Force

$path = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($path -split ";" | Where-Object { $_ })
if ($parts -notcontains $folder) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $folder) -join ";"), "User")
    Write-Host "Added $folder to your PATH. Open a new terminal to use it."
}
Write-Host "Installed $folder\moonowl.exe — run 'moonowl' in a new terminal."
