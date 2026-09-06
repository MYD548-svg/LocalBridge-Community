@echo off
cd /d "%~dp0"
title LocalBridge Community Build - Quick Start

where powershell.exe >nul 2>nul
if errorlevel 1 (
    echo [ERROR] powershell.exe not found.
    pause
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\quick-start.ps1"
if errorlevel 1 (
    echo.
    echo [INFO] Exited. Press any key to close...
    pause >nul
)