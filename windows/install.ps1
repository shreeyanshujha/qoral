# agora Windows installer (PowerShell, run from a clone of the repo that lives on the WSL filesystem
# or on Windows; either way the Linux side is what runs).
#
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1
#
# What it does:
#   1. checks WSL2 is installed and a distro exists
#   2. runs ./install.sh inside WSL (installs the Linux-side `agora` command)
#   3. copies agora.ps1 / agora.cmd to %LOCALAPPDATA%\agora\bin and adds that to your user PATH
# Afterwards `agora` works in PowerShell, cmd and Windows Terminal, running inside WSL.

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = Split-Path -Parent $here

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    Write-Host "WSL is not installed. Run:  wsl --install   then reboot and re-run this script." -ForegroundColor Red
    exit 1
}
$distros = (& wsl.exe -l -q 2>$null) -join ' '
if (-not $distros.Trim()) {
    Write-Host "No WSL distro found. Run:  wsl --install -d Ubuntu" -ForegroundColor Red
    exit 1
}
Write-Host "WSL distros: $($distros.Trim())"

# Translate the repo path to a WSL path (C:\x\y -> /mnt/c/x/y) unless it is already a \\wsl$ path.
if ($repo -like '\\wsl$\*' -or $repo -like '\\wsl.localhost\*') {
    $parts = $repo -replace '^\\\\wsl(\$|\.localhost)\\[^\\]+', ''
    $wslRepo = ($parts -replace '\\', '/')
} else {
    $wslRepo = & wsl.exe wslpath -a "$repo"
}
Write-Host "repo inside WSL: $wslRepo"

Write-Host "`nRunning the Linux installer inside WSL..." -ForegroundColor Cyan
& wsl.exe -- bash -lic "cd '$wslRepo' && sh ./install.sh"
if ($LASTEXITCODE -ne 0) {
    Write-Host "`nThe Linux installer reported problems (missing node/tmux?). Fix them inside WSL, e.g.:" -ForegroundColor Yellow
    Write-Host "  sudo apt update && sudo apt install -y tmux"
    Write-Host "  curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - && sudo apt install -y nodejs"
    exit 1
}

$binDir = Join-Path $env:LOCALAPPDATA 'agora\bin'
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Copy-Item (Join-Path $here 'agora.ps1') $binDir -Force
Copy-Item (Join-Path $here 'agora.cmd') $binDir -Force

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $binDir) {
    [Environment]::SetEnvironmentVariable('Path', "$binDir;$userPath", 'User')
    Write-Host "added $binDir to your user PATH (open a new terminal to pick it up)" -ForegroundColor Green
} else {
    Write-Host "$binDir already on PATH" -ForegroundColor Green
}

Write-Host "`nDone. In a new PowerShell / Windows Terminal window run:  agora doctor   then:  agora" -ForegroundColor Green
Write-Host "Tip: Windows Terminal binds Alt+arrows to its own pane focus; use Alt+h / Alt+l inside agora." -ForegroundColor DarkGray
