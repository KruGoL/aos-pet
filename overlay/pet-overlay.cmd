@echo off
rem Launcher for the aos-pet desktop overlay. Pin me to Start/taskbar.
cd /d "%~dp0"
if not exist node_modules call npm install
call npm start
