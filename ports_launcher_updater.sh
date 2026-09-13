#!/bin/sh
printf '\033]0;Ports Launcher Updater\007'
cd "$(dirname "$(readlink -f "$0")")" || exit 1

pkill -x ports_launcher
sleep 2

echo "Downloading latest version..."
TMPFILE="$(mktemp /tmp/PortsLauncher-update.XXXXXX.tar.gz)"
curl -L -o "$TMPFILE" "https://github.com/Nyaldee/Ports-Launcher/releases/latest/download/Ports.Launcher.Linux.tar.gz" || { echo "Download failed."; read -r _; exit 1; }

echo "Installing..."
tar -xf "$TMPFILE" -C .. --exclude="Ports Launcher/ports_launcher_updater.sh" || { echo "Extraction failed."; read -r _; exit 1; }
rm -f "$TMPFILE"
chmod +x ports_launcher 2>/dev/null

echo "Refreshing catalog..."
curl -fsSL -o /tmp/ports.json.new "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/ports.json" && mv -f /tmp/ports.json.new ports.json
curl -fsSL -o /tmp/themes.json.new "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/themes.json" && mv -f /tmp/themes.json.new themes.json

if [ -x ports_launcher ]; then
    nohup ./ports_launcher >/dev/null 2>&1 &
else
    echo
    echo "Move this file into the \"Ports Launcher\" folder."
    read -r _
fi
