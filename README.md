# Ports Launcher

<p align="center">
  <img src="Ports Launcher.webp" alt="Ports Launcher screenshot">
</p>

*[Lire en français](README.fr.md) · [日本語で読む](README.jp.md) · [阅读中文版](README.zh-CN.md)*

> [!IMPORTANT]
> **Ports Launcher itself ships no game files, ROMs, ISOs, or any other copyrighted asset whatsoever.** Every catalog entry only ever installs the "recomp"/source-port *code* — an open-source build published by that project's own repository — except Steam entries, which Steam itself installs from your own account. Any original game data an entry needs is yours to supply, from your own legally-obtained copy; Ports Launcher never bundles, downloads, hosts, or links to that data anywhere.

A lightweight library/installer for unofficial game ports: browse a catalog, install straight from a GitHub/GitLab/Forgejo release (or a direct download link), launch, and keep everything up to date from one window. Built with Rust and [Slint](https://slint.dev), with full keyboard *and* gamepad navigation throughout, and nothing to install separately (a minimal 7-Zip ships alongside the executable to extract port releases).

Want to see your project in the catalog? Ask on the [Discord server](https://ports-launcher.nyaruhodo.com/discord).

## Features

- Catalog-driven library (`catalog.json`), merged with your own catalogs (any `.json` with `catalog` in its name)
- One-click install: downloads the right release asset for your architecture automatically
- Auto-updates by default — turn it off per port from the Info panel's **Update** button
- Ports Launcher checks for new versions of itself too — the **GitHub** button in the footer turns into **Update** when a newer build is out; updates can be turned off, or forced immediately, from ◯ Settings
- Info panel per port: installed version/tag, setup instructions, and one-click links to the website, mods page, install folder, and save folder(s) — with fully selectable/copyable instructions text
- **Select version** button in the Info panel — pick from the last few GitHub/GitLab/Forgejo releases and install one instead of always the latest, or force an update right away; also turns off auto-update for that port, re-enable it anytime
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

Next to the version text, a **Select version** button (GitHub/GitLab/Forgejo ports only) fetches the last few releases and lets you install any of them instead of always the latest — handy when the newest release doesn't have a build for your platform, or you just want to roll back. It also always fetches fresh from GitHub/GitLab/Forgejo, so picking the latest one from the list is a way to force an update right away; installing a specific version this way also turns off auto-update for that port, so it isn't silently swapped for the latest release on the next **Play**.

For an installed port, the row also shows **Auto-update: On/Off**, **Favorite executable**, **Last played**, and **Play time**, each with its own button right below the version/status text — turn auto-update off (and back on) per port, pick which executable **Play** launches directly without asking every time, or reset that port's tracked playtime. A port with auto-update turned off shows a crossed-out yellow **Update** button next to **Play** in the main list as a reminder that it won't update itself.

Next to those, an **Install extras** button installs optional preset configurations for the port when available, overwriting any existing ones — never pulled in automatically by **Play**/**Install**, always opt-in from here.

## Game Saves

Ports Launcher handles two separate save mechanisms, both based on `catalog.json`'s `save`/`save2` fields, kept in a `Saves Backup` folder next to the executable:

**Automatic preservation** (uninstall/reinstall) — uninstalling a port whose save lives inside the install folder copies it to `Saves Backup/Pending Restore/<folder>/save_folder/` (or `.../save_folder2/` for the second save location) a moment before the rest of the folder is deleted, then a later install of that same port moves it straight back into place and removes this temporary copy. A save that sits outside the install folder (e.g. under `%APPDATA%`) is never touched by an uninstall either way — it already survives on its own. `Pending Restore` is never a history: a single slot per port/field, overwritten on every uninstall — worth knowing about if you're digging through `Saves Backup/` by hand for a save that seems to have vanished mid-reinstall, or if an install gets interrupted and you need to recover it manually.

**Manual export** (**Save Backup** button, see ◯ [Settings](#settings) right below) — on demand, exports every port's saves across the whole catalog (installed or not, external or local) into `Saves Backup/Global Backups/<date>.zip`. Running it again the same day overwrites that day's `.zip` with the current state rather than merging into it, so a port you've since uninstalled doesn't linger in it — each date is a clean snapshot, an actual history unlike `Pending Restore`.

## Settings

Open it from the **◯** button in the title bar — a menu with **Themes**, **Language**, **Library**, **Tools**, **Save Backup**, **Check for Updates**, **Announcements**, **Discord Rich Presence**, **Custom Rich Presence**, **Catalogs**, and **Force Update**.

- **Themes**, **Language**, and **Tools** all open the same kind of live, fuzzy-searchable picker. For Themes, moving the selection (mouse hover, or `↑`/`↓`/the controller stick) previews it instantly across the whole app; confirming writes straight back to `themes.json`, and closing without confirming (`Escape`) reverts to whichever theme was active before. Language switches the UI immediately on selection, no restart needed.
- **Tools** lists handy external tools for preparing the files ports ask for (archivers, patchers, disc extractors, converters...), searchable by name or by tag (`patcher`, `psp`, `360`...). Picking one opens its page in your browser — you download and set it up yourself. The list comes from the `"tools"` section of `catalog.json`, so it stays up to date with catalog updates, plus the tools of your own catalogs.
- **Catalogs** lists the detected catalogs, each with an On/Off switch (`catalog.json` included). The library updates right away; a port from a catalog turned off disappears from it, but its files stay in `Library/` and it comes back as installed when the catalog is turned on again.
- **Library** jumps straight to that folder in Explorer.
- **Save Backup** kicks off a full save export for the whole catalog into a dated `.zip` (see [Game Saves](#game-saves) above), with a progress window while it copies.
- **Check for Updates** toggles On/Off right in the menu — turns every update check off at once (the launcher's own, and every installed port's at **Play**), for anyone who'd rather update everything by hand instead.
- **Force Update** runs the launcher's own updater right away, without checking first whether a newer build actually exists — for anyone who doesn't want to wait for the periodic check (or has it turned off above).
- **Discord Rich Presence** toggles On/Off right in the menu, off by default — shows the game you're playing on your Discord status while it's running, and clears itself the moment you close it.
- **Custom Rich Presence** toggles On/Off right in the menu, off by default — shows your own status instead, for as long as Ports Launcher is running, with elapsed time and the **Ports Launcher** button; games no longer show their own presence while it's on. Set its text and icon in `state.json` (with Ports Launcher closed, or your edit gets overwritten):

  ```json
  "custom_presence": {
    "enabled": true,
    "text": "Your text",
    "icon": "https://... (public image URL, optional)"
  }
  ```

  Without a `text`, it does nothing and games keep their presence as usual. **Discord Rich Presence** must be on too.

- **Announcements** toggles On/Off right in the menu, on by default — shows messages from the maintainer in a banner to the left of the footer buttons, in windowed mode only. The text only scrolls if it doesn't fit in the banner, and if the message has a link, clicking the banner opens it in your browser. When off, Ports Launcher doesn't fetch them at all.

## Configuration

### `catalog.json`

The main catalog. Not meant to be hand-edited: it's replaced wholesale by catalog updates, so anything you add here yourself gets silently overwritten the next time it refreshes — put your own ports in another catalog instead (see below).

### Your own catalogs

Any `.json` file next to Ports Launcher with `catalog` in its name (`catalog.local.json`, `my-catalog.json`...) is a catalog too. It uses exactly the same format as `catalog.json` — a `"ports"` list and a `"tools"` list — and everything is merged into a single library and a single Tools list. By default, only `catalog.json` is downloaded, plus any catalogs you add to `"catalog_urls"` (see [`state.json`](#statejson)), so your own catalogs survive every catalog update.

- **Order:** `catalog.json` first, then the other catalogs in alphabetical order, 10 files at most. An entry replaces the one with the same `folder` (ports) or the same `name` (tools) read before it, so a port of yours with the same `folder` as an official one replaces it.
- **Ports without `source`:** for games you install yourself — Install creates the folder under `Library/`, you put the game's files there, and it plays like any other port.
- **Unreadable file:** skipped, the rest still loads.
- **External catalogs:** add their URLs to `"catalog_urls"` in `state.json` (see below): each one is downloaded next to Ports Launcher and kept up to date. You can also save one there yourself (any name with `catalog` in it), but it is not kept up to date. Some external catalogs use their own format, recognized by its content: those only *add* games — a project already in your other catalogs is skipped, so your other catalogs always win — and only the projects published for your system are listed.
- **Turned off in Settings > Catalogs:** not loaded at all. Turned-off catalogs are listed under `"disabled_catalogs"` in `state.json`; a newly added catalog is on.
- **Steam games:** a `source` like `https://store.steampowered.com/app/620/` is a Steam game; `folder` isn't needed. Installed games are detected on every drive, whether Steam is running or not. **Install** opens its page in Steam, **Play** launches it through Steam, and **Uninstall** hands over to Steam. Extras and save folders work as usual; versions, updates, playtime, and Discord presence are left to Steam.
- **`source_type`:** how `source` is read. Without it, a `github.com`, `gitlab.com` or `codeberg.org` repository link is recognized as such, and any other link is a direct download. `"forgejo"` reads `source` as a repository on any Forgejo instance (`https://host/owner/repo`), and `"direct"` forces a direct download. An unknown value makes Ports Launcher skip the entry.

Ports Launcher creates `catalog.local.json` as an example if it doesn't exist yet, with a disabled port and tool (`"-name"` instead of `"name"`, which makes Ports Launcher skip them) — rename `"-name"` to `"name"`, fill in real values, and it becomes a real entry. An existing `ports.local.json` from an older version is renamed automatically.

### On Android

Settings → Catalogs works the same way, with a few differences:

- **Catalog folder:** pick a folder on your device; its `.json` files with `catalog` in their name are read. An entry replaces the one with the same `name`. External catalogs in their own format aren't supported.
- **Manage installed apps:** check your installed apps to add them to `catalog.local.json`, uncheck one to turn it off, long-press it (or **Y** on a gamepad) to delete it. The entries you write by hand in this file are never touched, as long as they don't contain `"android_managed"`.
- **Export / Import configuration** (Settings) saves your settings and playtime to a file, to restore them on a new phone for example. Catalogs aren't included: they're already files in your folder.

### `state.json`

Ports Launcher's configuration file — installed versions/playtime, window state, display preferences, and update-check timestamps, written automatically as you use the app.

You can also add a `"github_token"`/`"gitlab_token"` key by hand to raise the GitHub/GitLab API rate limit for update checks. For Forgejo, each instance has its own tokens: fill in `"forgejo_tokens"` with one token per host, like `{"codeberg.org": "..."}` — a token is only ever sent to its own host. That token is then stored in **plain text** — never share this file, upload it anywhere, or leave it visible on a stream/screen share.

To fetch the catalog, themes, or announcements from another address, fill in the `"catalog_urls"`, `"themes_url"`, or `"announcement_url"` key (with Ports Launcher closed, or your edit gets overwritten). They must be `https://` links. Left empty, Ports Launcher uses its default addresses. `"catalog_urls"` is a list of catalogs to follow in addition to the default one, which is always downloaded: `"catalog_urls": ["https://example.com/catalog.json", "https://example.org/games.json"]`. Each address gets its own file (`catalog.json` for the default catalog, a name taken from the address for the others) and is updated on its own. Removing an address leaves its file in place: delete it, or turn it off in Settings > Catalogs.

## Credits

- [SteamGridDB](https://www.steamgriddb.com/) for the box art used by catalog entries
- The creators of the recomps/source-ports listed in `catalog.json` — often months of unpaid reverse-engineering work, without which none of this would exist
- [7-Zip](https://github.com/ip7z/7zip), a minimal command-line copy of which ships with Ports Launcher to extract port release archives

## License

Copyright (C) 2026 Nyaldee. Licensed under the [GNU General Public License v3.0](LICENSE) — see the `LICENSE` file for the full text.

[Claude](https://claude.com), Anthropic's AI, contributed to the code of this project.
