@echo off
color 0A
cd /d "%~dp0"

taskkill /IM "ports_launcher.exe" /F >nul 2>&1
timeout /t 2 /nobreak >nul

echo Downloading latest version...
curl -L -o "%TEMP%\PortsLauncher-update.zip" "https://github.com/Nyaldee/Ports-Launcher/releases/latest/download/Ports.Launcher.zip" || (echo Download failed. & pause & exit /b 1)

echo Installing...
tar -xf "%TEMP%\PortsLauncher-update.zip" -C .. --exclude="Ports Launcher/ports_launcher_updater.bat" || (echo Extraction failed. & pause & exit /b 1)

del /q "%TEMP%\PortsLauncher-update.zip"

echo Refreshing catalog...
curl -fsSL -o "%TEMP%\ports.json.new" "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/ports.json" && move /y "%TEMP%\ports.json.new" "ports.json" >nul
curl -fsSL -o "%TEMP%\themes.json.new" "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/themes.json" && move /y "%TEMP%\themes.json.new" "themes.json" >nul

if exist "ports_launcher.exe" (
    start "" "ports_launcher.exe"
) else (
    echo.
    echo Move this file into the "Ports Launcher" folder.
    pause
)
