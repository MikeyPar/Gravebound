@echo off
cd /d "%~dp0"
server_app.exe serve --bind 127.0.0.1:50000 --content-root content --certificate-out server-cert.der
if errorlevel 1 pause
