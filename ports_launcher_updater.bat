@echo off
title Ports Launcher Updater & color 0A
cd /d "%~dp0"

set "REPO=https://github.com/Nyaldee/Ports-Launcher"
set "RAW=https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main"
set "ARCHIVE=%TEMP%\PortsLauncher-update.zip"

taskkill /IM "ports_launcher.exe" /F >nul 2>&1
timeout /t 2 /nobreak >nul

set "EXTRACT_TO=."
set "INSTALL_DIR=Ports Launcher"
for %%I in ("%CD%") do if /I "%%~nxI"=="Ports Launcher" (set "EXTRACT_TO=.." & set "INSTALL_DIR=.")
set "NEW_SELF=%INSTALL_DIR%\%~nx0.new"

echo Downloading latest version...
curl -fL -o "%ARCHIVE%" "%REPO%/releases/latest/download/Ports.Launcher.Windows.zip" || (echo Download failed. & pause & exit /b 1)

echo Installing...
tar -xf "%ARCHIVE%" -C "%EXTRACT_TO%" --exclude="Ports Launcher/%~nx0" || (echo Extraction failed. & pause & exit /b 1)
tar -xOf "%ARCHIVE%" "Ports Launcher/%~nx0" > "%NEW_SELF%" 2>nul
del /q "%ARCHIVE%"

echo Refreshing catalog...
curl -fsSL -o "%TEMP%\ports.json.new" "%RAW%/ports.json" && move /y "%TEMP%\ports.json.new" "%INSTALL_DIR%\ports.json" >nul
curl -fsSL -o "%TEMP%\themes.json.new" "%RAW%/themes.json" && move /y "%TEMP%\themes.json.new" "%INSTALL_DIR%\themes.json" >nul

start "" "%INSTALL_DIR%\ports_launcher.exe"

(
    for %%F in ("%NEW_SELF%") do if %%~zF GTR 0 (
        move /y "%NEW_SELF%" "%INSTALL_DIR%\%~nx0" >nul
        if /I not "%INSTALL_DIR%"=="." del "%~f0"
    ) else (
        del /q "%NEW_SELF%"
        if /I not "%INSTALL_DIR%"=="." move /y "%~f0" "%INSTALL_DIR%\%~nx0" >nul
    )
    exit /b 0
)
