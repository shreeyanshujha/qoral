@echo off
rem agora launcher for cmd.exe: forwards to the PowerShell launcher, which runs agora inside WSL2.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0agora.ps1" %*
