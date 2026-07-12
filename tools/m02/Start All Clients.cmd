@echo off
cd /d "%~dp0"
if not exist server-cert.der (
  echo Start the server first and wait for server-cert.der to appear.
  pause
  exit /b 1
)
start "Gravebound Client 1" "%~dp0client_bevy.exe" network --server 127.0.0.1:50000 --certificate "%~dp0server-cert.der" --player local-player-1 --content-root "%~dp0content"
start "Gravebound Client 2" "%~dp0client_bevy.exe" network --server 127.0.0.1:50000 --certificate "%~dp0server-cert.der" --player local-player-2 --content-root "%~dp0content"
start "Gravebound Client 3" "%~dp0client_bevy.exe" network --server 127.0.0.1:50000 --certificate "%~dp0server-cert.der" --player local-player-3 --content-root "%~dp0content"
start "Gravebound Client 4" "%~dp0client_bevy.exe" network --server 127.0.0.1:50000 --certificate "%~dp0server-cert.der" --player local-player-4 --content-root "%~dp0content"
