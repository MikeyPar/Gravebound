@echo off
cd /d "%~dp0"
client_bevy.exe network --server 127.0.0.1:50000 --certificate server-cert.der --player local-player-3 --content-root content
if errorlevel 1 pause
