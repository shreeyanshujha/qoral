# agora launcher for PowerShell / Windows Terminal.
# agora runs inside WSL2 (it needs tmux). This forwards the command line to it.
#   agora                 -> opens the workspace
#   agora spawn claude "task" --dir ~/proj
#   agora debate "question" --dir ~/proj --build
#
# Set AGORA_WSL_DISTRO to pick a distro other than the default.

$ErrorActionPreference = 'Stop'

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    Write-Error "WSL is not installed. Run:  wsl --install   (then reboot) and install agora inside it."
    exit 1
}

# Quote each argument for bash inside WSL.
$quoted = $args | ForEach-Object {
    "'" + ($_ -replace "'", "'\''") + "'"
}
$cmd = "agora " + ($quoted -join ' ')

$wslArgs = @()
if ($env:AGORA_WSL_DISTRO) { $wslArgs += @('-d', $env:AGORA_WSL_DISTRO) }
# -lic: login + interactive shell so ~/.local/bin and mise/nvm PATH setup apply.
$wslArgs += @('--', 'bash', '-lic', $cmd)

& wsl.exe @wslArgs
exit $LASTEXITCODE
