#!/bin/sh
printf '\033]0;Ports Launcher Updater\007'
cd "$(dirname "$(readlink -f "$0")")" || exit 1

REPO="https://github.com/Nyaldee/Ports-Launcher"
RAW="https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main"
SELF="$(basename "$0")"

fail() {
    echo "$1"
    read -r _
    exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required but not installed -- install it (e.g. \"sudo apt install curl\") and try again."

pkill -x ports_launcher
sleep 2

# L'archive contient un dossier "Ports Launcher/" : depuis ce dossier, on
# extrait dans le parent ; depuis ailleurs, le dossier est créé ici.
EXTRACT_TO=.
INSTALL_DIR="Ports Launcher"
if [ "$(basename "$PWD")" = "Ports Launcher" ]; then
    EXTRACT_TO=..
    INSTALL_DIR=.
fi

TMP_DIR="$(mktemp -d)" || fail "Couldn't create a temporary folder."
trap 'rm -rf "$TMP_DIR"' EXIT
ARCHIVE="$TMP_DIR/update.tar.gz"

echo "Downloading latest version..."
curl -fL -o "$ARCHIVE" "$REPO/releases/latest/download/Ports.Launcher.Linux.tar.gz" || fail "Download failed."

echo "Installing..."
tar -xf "$ARCHIVE" -C "$EXTRACT_TO" --exclude="Ports Launcher/$SELF" || fail "Extraction failed."
chmod +x "$INSTALL_DIR/ports_launcher" "$INSTALL_DIR/7zzs" 2>/dev/null
NEW_SELF="$INSTALL_DIR/.$SELF.new"
tar -xOf "$ARCHIVE" "Ports Launcher/$SELF" > "$NEW_SELF" 2>/dev/null

echo "Refreshing catalog..."
curl -fsSL -o "$TMP_DIR/ports.json" "$RAW/ports.json" && mv -f "$TMP_DIR/ports.json" "$INSTALL_DIR/ports.json"
curl -fsSL -o "$TMP_DIR/themes.json" "$RAW/themes.json" && mv -f "$TMP_DIR/themes.json" "$INSTALL_DIR/themes.json"

nohup "$INSTALL_DIR/ports_launcher" >/dev/null 2>&1 &

# Renommage dans le même dossier : le shell continue de lire l'ancien
# fichier, déjà ouvert, jusqu'à la fin du script.
if [ -s "$NEW_SELF" ]; then
    chmod +x "$NEW_SELF"
    mv -f "$NEW_SELF" "$INSTALL_DIR/$SELF"
    [ "$INSTALL_DIR" = . ] || rm -f "$0"
else
    rm -f "$NEW_SELF"
    [ "$INSTALL_DIR" = . ] || mv -f "$0" "$INSTALL_DIR/$SELF"
fi
