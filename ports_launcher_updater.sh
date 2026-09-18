#!/bin/sh
printf '\033]0;Ports Launcher Updater\007'
cd "$(dirname "$(readlink -f "$0")")" || exit 1

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required but not installed -- install it (e.g. \"sudo apt install curl\") and try again."
    read -r _
    exit 1
fi

pkill -x ports_launcher
sleep 2

echo "Downloading latest version..."
TMPFILE="$(mktemp /tmp/PortsLauncher-update.XXXXXX.tar.gz)"
curl -L -o "$TMPFILE" "https://github.com/Nyaldee/Ports-Launcher/releases/latest/download/Ports.Launcher.Linux.tar.gz" || { echo "Download failed."; read -r _; exit 1; }

# L'archive contient elle-même un dossier "Ports Launcher/" -- extraire dans
# le PARENT recrée/écrase ce même dossier d'install en place, tant que ce
# script tourne déjà depuis l'intérieur (INSTALL_DIR vaut alors "." lui-
# même). Lancé depuis ailleurs, ce dossier apparaît ici même comme
# INSTALL_DIR. --exclude reste toujours actif : la copie de ce script en
# cours d'exécution est forcément plus récente que celle de la release
# téléchargée, jamais remplacée par elle -- relocalisée à sa place en tout
# dernier à la place.
EXTRACT_TO=.
INSTALL_DIR="Ports Launcher"
if [ "$(basename "$PWD")" = "Ports Launcher" ]; then
    EXTRACT_TO=..
    INSTALL_DIR=.
fi

echo "Installing..."
tar -xf "$TMPFILE" -C "$EXTRACT_TO" --exclude="Ports Launcher/ports_launcher_updater.sh" || { echo "Extraction failed."; read -r _; exit 1; }
rm -f "$TMPFILE"
chmod +x "$INSTALL_DIR/ports_launcher" "$INSTALL_DIR/7zzs" 2>/dev/null

echo "Refreshing catalog..."
curl -fsSL -o /tmp/ports.json.new "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/ports.json" && mv -f /tmp/ports.json.new "$INSTALL_DIR/ports.json"
curl -fsSL -o /tmp/themes.json.new "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/themes.json" && mv -f /tmp/themes.json.new "$INSTALL_DIR/themes.json"

nohup "$INSTALL_DIR/ports_launcher" >/dev/null 2>&1 &
mv -f "$0" "$INSTALL_DIR/$(basename "$0")"
