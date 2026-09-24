# Ports Launcher

<p align="center">
  <img src="Ports Launcher.jpg" alt="Ports Launcher screenshot">
</p>

*[Lire en français](README.fr.md)*

> [!IMPORTANT]
> **Ports Launcher itself ships no game files, ROMs, ISOs, or any other copyrighted asset whatsoever.** Every catalog entry only ever installs the "recomp"/source-port *code* — an open-source build published by that project's own repository. Any original game data an entry needs is yours to supply, from your own legally-obtained copy; Ports Launcher never bundles, downloads, hosts, or links to that data anywhere.

A lightweight library/installer for unofficial game ports: browse a catalog, install straight from a GitHub/GitLab release (or a direct download link), launch, and keep everything up to date from one window. Built with Rust and [Slint](https://slint.dev), with full keyboard *and* gamepad navigation throughout, and nothing to install separately (a minimal 7-Zip ships alongside the executable to extract port releases).

## Features

- Catalog-driven library (`ports.json`), plus your own local catalog (`ports.local.json`) for ports you manage yourself
- One-click install: downloads the right release asset for your architecture automatically
- Auto-updates by default — turn it off per port from the Info panel's **Update** button
- Ports Launcher checks its own GitHub releases too — the **GitHub** button in the footer turns into **Update** when a newer build is out; updates can be turned off, or forced immediately, from ◯ Settings
- Info panel per port: installed version/tag, setup instructions, and one-click links to the website, mods page, install folder, and save folder(s) — with fully selectable/copyable instructions text
- **Select version** button in the Info panel — pick from the last few GitHub/GitLab releases and install one instead of always the latest, or force an update right away; also turns off auto-update for that port, re-enable it anytime
- **Install extras** button in the Info panel — installs optional preset configurations for a port when available, overwriting any existing ones
- Uninstall in one click, with your save data preserved if it lives inside the port's own folder; box art is downloaded once and cached locally afterward
- **Save Backup** button in ◯ Settings to export every port's saves into a dated `.zip` in one click (see [Game Saves](#game-saves) for both mechanisms)
- Playtime tracking — every port's cumulated play time and last-played date show up in its Info panel, with a **Reset Game Time** button if you want to start the counter over
- The catalog lists your most recently played ports first
- **Discord Rich Presence** switch in ◯ Settings (off by default) — shows the game you're playing on your Discord profile while it's running
- Fullscreen "Library" mode (`Alt+Enter`) — every *installed* port as a grid of cards, Steam Big Picture style
- Full gamepad navigation (XInput) *and* full keyboard navigation (arrow keys, Enter, Escape) everywhere in the app, including every dialog — browse, install, launch, open info, pick a file/executable, and back out, with either a controller or just the keyboard
- 100+ ready-made color themes, switchable live from the in-app ◯ Settings picker with instant preview
- System tray icon — closing the main window sends it to the tray instead of quitting. Left-click the icon to bring it back; right-click for a menu with your 5 most recently played games (jumps straight to **Play**), **Settings**, and **Quit**. A dedicated minimize button sits in the title bar too, next to ◯ Settings

## Keyboard & Gamepad

| Key ⌨️ | Button 🎮 | Action |
|---|---|---|
| Type | — | Fuzzy-filter the catalog live |
| `↑` / `↓` or `Ctrl+W` / `Ctrl+S` | D-pad / left stick | Move selection up / down |
| `←` / `→` or `Ctrl+A` / `Ctrl+D` | D-pad / left stick | Jump a page (10 rows, windowed list) / a column (Library grid) |
| `Enter` | `A` or `Start` | Install the selected port if it isn't installed yet, launch it otherwise |
| `Shift+Enter` | — | Open the selected port's install folder in Explorer |
| — | `X` | Open the Info panel for the selected port |
| — | `Y` | Open ◯ Settings |
| `Alt+Enter` | `Back` | Toggle fullscreen Library mode |
| `Ctrl+1`...`Ctrl+9` / `Ctrl+0` | — | Resize the windowed launcher to 10%...90% / 100% of screen size, windowed mode only |
| `Ctrl+-` / `Ctrl+=` | — | Shrink / grow the border by 1px, windowed mode only |
| `Escape` | `B` | Close the dialog on top if one is open, back out of Library mode if it's active, otherwise close the launcher (`B` alone only backs out of a dialog) |

Plug in an XInput controller (Xbox-style) and it works immediately, no setup needed, right alongside the keyboard. The search box always keeps focus in the main window — every key/button above is intercepted there directly. Any dialog on top (Info, ◯ Settings, install progress, a file/executable picker...) gets its own focus instead: the same keys/buttons move the selection inside it, `Enter`/`A` activates it, `Escape`/`B` closes it (except the install-progress dialog, which can't be interrupted). Moving the selection with the mouse and with the keyboard/controller always stays in sync — only one thing is ever highlighted at a time, however you moved it there.

You can also filter the search by tag to find games. A few worth knowing: `free` for games that are **legally** free (open-source or freeware), `quality` for ports considered especially polished/well put together, and `online` for ports with online multiplayer. The rest of the catalog's tags work the same way — platform of origin (`n64`, `ps1`, `snes`, `360`, `gc`...), genre (`platformer`, `rpg`, `fighting`...), `multiplayer`, `multilingual`, and so on.

## Library mode

`Alt+Enter` switches to a fullscreen grid of every port you currently have *installed* — like Steam's Big Picture library. Ports you haven't installed yet only show up in the windowed list view, never in Library mode. `Escape` (or `Alt+Enter` again) returns to the windowed view.

## Info panel

Select a port and open its **Info** panel (button, or `X` on a controller) for its installed version/tag, any setup instructions (selectable, copy-pastable text), and one-click links to its website, mods page, install folder, and save folder — plus a **Save folder 2** button for ports with a second, independent save location (`save2`). Any of these is simply disabled if it doesn't exist yet (not installed, the game hasn't created a save yet, or the port has no second save location at all). `↑`/`↓` (or the controller D-pad/stick) scrolls the instructions text when it's too long to fit; `←`/`→` moves between the buttons, `Enter`/`A` activates whichever one is highlighted.

Next to the version text, a **Select version** button (GitHub/GitLab ports only) fetches the last few releases and lets you install any of them instead of always the latest — handy when the newest release doesn't have a build for your platform, or you just want to roll back. It also always fetches fresh from GitHub/GitLab, so picking the latest one from the list is a way to force an update right away; installing a specific version this way also turns off auto-update for that port, so it isn't silently swapped for the latest release on the next **Play**.

For an installed port, the row also shows **Auto-update: On/Off**, **Favorite executable**, **Last played**, and **Play time**, each with its own button right below the version/status text — turn auto-update off (and back on) per port, pick which executable **Play** launches directly without asking every time, or reset that port's tracked playtime. A port with auto-update turned off shows a crossed-out yellow **Update** button next to **Play** in the main list as a reminder that it won't update itself.

Next to those, an **Install extras** button installs optional preset configurations for the port when available, overwriting any existing ones — never pulled in automatically by **Play**/**Install**, always opt-in from here.

## Game Saves

Ports Launcher handles two separate save mechanisms, both based on `ports.json`'s `save`/`save2` fields, kept in a `Saves Backup` folder next to the executable:

**Automatic preservation** (uninstall/reinstall) — uninstalling a port whose save lives inside the install folder copies it to `Saves Backup/Pending Restore/<folder>/save_folder/` (or `.../save_folder2/` for the second save location) a moment before the rest of the folder is deleted, then a later install of that same port moves it straight back into place and removes this temporary copy. A save that sits outside the install folder (e.g. under `%APPDATA%`) is never touched by an uninstall either way — it already survives on its own. `Pending Restore` is never a history: a single slot per port/field, overwritten on every uninstall — worth knowing about if you're digging through `Saves Backup/` by hand for a save that seems to have vanished mid-reinstall, or if an install gets interrupted and you need to recover it manually.

**Manual export** (**Save Backup** button, see ◯ [Settings](#settings) right below) — on demand, exports every port's saves across the whole catalog (installed or not, external or local) into `Saves Backup/Global Backups/<date>.zip`. Running it again the same day overwrites that day's `.zip` with the current state rather than merging into it, so a port you've since uninstalled doesn't linger in it — each date is a clean snapshot, an actual history unlike `Pending Restore`.

## Settings

Open it from the **◯** button in the title bar — a menu with **Themes**, **Language**, **Files**, **Library**, **Save Backup**, **Check for Updates**, **Force Update**, and **Discord Rich Presence**.

- **Themes** and **Language** both open the same kind of live, fuzzy-searchable picker. For Themes, moving the selection (mouse hover, or `↑`/`↓`/the controller stick) previews it instantly across the whole app; confirming writes straight back to `themes.json`, and closing without confirming (`Escape`) reverts to whichever theme was active before. Language switches the UI immediately on selection, no restart needed.
- **Files** opens shortcuts to `ports.json`, `ports.local.json`, `state.json`, and `themes.json`, which do nothing if the file doesn't exist yet.
- **Library** jumps straight to that folder in Explorer.
- **Save Backup** kicks off a full save export for the whole catalog into a dated `.zip` (see [Game Saves](#game-saves) above), with a progress window while it copies.
- **Check for Updates** toggles On/Off right in the menu — turns every update check off at once (the launcher's own, and every installed port's at **Play**), for anyone who'd rather update everything by hand instead.
- **Force Update** runs the launcher's own updater right away, without checking first whether a newer build actually exists — for anyone who doesn't want to wait for the periodic check (or has it turned off above).
- **Discord Rich Presence** toggles On/Off right in the menu, off by default — shows the game you're playing on your Discord status while it's running, and clears itself the moment you close it.

## Useful tools

A couple of external tools come up repeatedly in `ports.json`'s **Required files** instructions, for preparing your own game data before pointing a port at it:

- **[7-Zip](https://github.com/ip7z/7zip)** — a minimal, command-line-only copy ships with Ports Launcher, but only to extract port release archives internally; get the regular version separately to handle archives as needed.
- **[extract-xiso](https://github.com/XboxDev/extract-xiso)** — not bundled, get it separately: for extracting the assets from your own Xbox/Xbox 360 dumps.

## Configuration

### `ports.json`

The main catalog. Not meant to be hand-edited: it's replaced wholesale by catalog updates, so anything you add here yourself gets silently overwritten the next time it refreshes — see [`ports.local.json`](#portslocaljson) below to add your own ports permanently instead.

### `ports.local.json`

Your own catalog, next to `ports.json` — for ports you manage yourself rather than install through Ports Launcher: you create the folder under `Library/`, put the game's files there by hand, and it shows up exactly like any other port (playable, has an Info panel, shows up in Library mode once installed). Same entry format as `ports.json`, except `source` never applies here — omit it, or set it to nothing. Since this file lives separately, replacing `ports.json` with a newer version from the maintainer never touches what you've added here.

A `folder` that collides with one from `ports.json` replaces the official entry with your own local one. The **Uninstall** button also behaves differently for a local port: it never deletes anything — it opens the port's folder in Explorer instead, so you stay in full control of files Ports Launcher never downloaded in the first place.

The repo ships a `ports.local.json` with two disabled example entries (`"name"`/`"folder"` set to `null`, which makes Ports Launcher skip them) — copy one, fill in real values, and it becomes a real entry.

### `state.json`

Ports Launcher's configuration file — installed versions/playtime, window state, display preferences, and update-check timestamps, written automatically as you use the app.

You can also add a `"github_token"`/`"gitlab_token"` key by hand to raise the GitHub/GitLab API rate limit for update checks. That token is then stored in **plain text** — never share this file, upload it anywhere, or leave it visible on a stream/screen share.

## Credits

- [SteamGridDB](https://www.steamgriddb.com/) for the box art used by catalog entries
- The creators of the recomps/source-ports listed in `ports.json` — often months of unpaid reverse-engineering work, without which none of this would exist
- [7-Zip](https://github.com/ip7z/7zip) and [extract-xiso](https://github.com/XboxDev/extract-xiso), the two external tools used/recommended for preparing required files (see [Useful tools](#useful-tools))

Built together with [Claude](https://claude.com) (Anthropic's AI coding assistant).

## License

Copyright (C) 2026 Nyaldee. Licensed under the [GNU General Public License v3.0](LICENSE) — see the `LICENSE` file for the full text.
