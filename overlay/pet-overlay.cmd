@echo off
rem Launcher for the aos-pet desktop overlay. Pin me to Start/taskbar.
rem If inherited from a Node-based parent, this makes Electron run as plain
rem Node and crash at startup — always clear it.
set ELECTRON_RUN_AS_NODE=
cd /d "%~dp0"
if not exist node_modules call npm install
call npm start
