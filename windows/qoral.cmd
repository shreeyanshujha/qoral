@echo off
rem qoral launcher for cmd.exe: forwards to the PowerShell launcher, which runs qoral inside WSL2.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0qoral.ps1" %*
