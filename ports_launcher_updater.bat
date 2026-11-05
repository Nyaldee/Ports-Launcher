@echo off
title Ports Launcher Updater & color 0A
cd /d "%~dp0"

set "DOWNLOAD_URL=https://ports-launcher.nyaruhodo.com/download/windows"
set "ARCHIVE=%TEMP%\PortsLauncher-update.zip"

taskkill /IM "ports_launcher.exe" /F >nul 2>&1
timeout /t 2 /nobreak >nul

set "EXTRACT_TO=."
set "INSTALL_DIR=Ports Launcher"
for %%I in ("%CD%") do if /I "%%~nxI"=="Ports Launcher" (set "EXTRACT_TO=.." & set "INSTALL_DIR=.")
set "NEW_SELF=%INSTALL_DIR%\%~nx0.new"

echo Downloading latest version...
curl -fL -o "%ARCHIVE%" "%DOWNLOAD_URL%" || (echo Download failed. & pause & exit /b 1)

echo Installing...
tar -xf "%ARCHIVE%" -C "%EXTRACT_TO%" --exclude="Ports Launcher/%~nx0" || (echo Extraction failed. & pause & exit /b 1)
tar -xOf "%ARCHIVE%" "Ports Launcher/%~nx0" > "%NEW_SELF%" 2>nul
del /q "%ARCHIVE%"

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
