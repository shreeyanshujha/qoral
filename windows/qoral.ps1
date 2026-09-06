# qoral launcher for PowerShell / Windows Terminal.
# qoral runs inside WSL2 (it needs tmux). This forwards the command line to it.
#   qoral                 -> opens the workspace
#   qoral spawn claude "task" --dir ~/proj
#   qoral debate "question" --dir ~/proj --build
#
# Set QORAL_WSL_DISTRO to pick a distro other than the default.

$ErrorActionPreference = 'Stop'

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    Write-Error "WSL is not installed. Run:  wsl --install   (then reboot) and install qoral inside it."
    exit 1
}

# Quote each argument for bash inside WSL.
$quoted = $args | ForEach-Object {
    "'" + ($_ -replace "'", "'\''") + "'"
}
$cmd = "qoral " + ($quoted -join ' ')

$wslArgs = @()
if ($env:QORAL_WSL_DISTRO) { $wslArgs += @('-d', $env:QORAL_WSL_DISTRO) }
# -lic: login + interactive shell so ~/.local/bin and mise/nvm PATH setup apply.
$wslArgs += @('--', 'bash', '-lic', $cmd)

& wsl.exe @wslArgs
exit $LASTEXITCODE
