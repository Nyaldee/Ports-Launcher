# Ports Launcher

<p align="center">
  <img src="Ports Launcher.jpg" alt="Ports Launcher screenshot">
</p>

*[Lire en français](README.fr.md)*

A lightweight library/installer for native "recomp" and source-port builds of console games, for Windows: browse a catalog, install straight from a GitHub/GitLab release (or a direct download link), launch, and keep everything up to date from one window. Built with Rust and [Slint](https://slint.dev), with full keyboard *and* gamepad navigation throughout, and nothing to install separately (a minimal 7-Zip ships alongside the executable to extract port releases).

**Ports Launcher itself ships no game files, ROMs, ISOs, or any other copyrighted asset whatsoever.** Every catalog entry only ever installs the "recomp"/source-port *code* — an open-source build published by that project's own repository. Any entry that needs original game data says so explicitly in its **Required files** instructions, and expects you to supply it yourself from a copy of the game you already legally own; Ports Launcher never bundles, downloads, hosts, or links to that data anywhere. Using your own original, legally-obtained copy is entirely on you — Ports Launcher has no way to check where that file came from, and takes no responsibility for it.

## Features

- Catalog-driven library (`ports.json`) — add a new port by editing one JSON file, no code changes needed
- **Your own local catalog** (`ports.local.json`) — add ports you manage yourself (no download source, you place the game files under `Library/` by hand) in a separate file that's never touched by a `ports.json` update, so your own additions always survive
- One-click install: downloads the right release asset for your architecture automatically (GitHub or GitLab releases, or a direct URL), extracts it, and unwraps a single top-level wrapper folder if the archive has one
- Per-port update detection — an installed port gets an **Update** button next to **Play** the moment a newer release is out, based on the release tag *and* its publish/asset date (so a project that keeps recycling the same "latest" tag for every build is still caught)
- Ports Launcher checks its own GitHub releases too: the **GitHub** button in the footer turns into **Update** when a newer build of the launcher itself is out. Clicking it still just opens the GitHub page — nothing is downloaded or replaced automatically, you grab and install the new build yourself
- Uninstall in one click, with your save data preserved if it lives inside the port's own folder — a save that sits outside `Library/` (e.g. under `%APPDATA%`) is never touched either way; box art is downloaded once and cached locally afterward
- Info panel per port: installed version/tag, setup instructions, and one-click links to the website, mods page, install folder, and save folder(s) — with fully selectable/copyable instructions text
- **Change version** button in the Info panel — pick from the last few GitHub/GitLab releases and install that one instead of always the latest, useful when the newest release drops a platform you need (e.g. a Windows build)
- Fullscreen "Library" mode (`Alt+Enter`) — every *installed* port as a grid of cards, Steam Big Picture style
- Full gamepad navigation (XInput) *and* full keyboard navigation (arrow keys, Enter, Escape) everywhere in the app, including every dialog — browse, install, launch, open info, pick a file/executable, and back out, with either a controller or just the keyboard
- Every dialog matches the main window's title bar, font, and sizing, and grows to fit its own content — a long message or a long filename is never cut off
- 100+ ready-made color themes, switchable live from the in-app Settings picker with instant preview (same `themes.json` format as [MAGI Launcher](https://github.com/Nyaldee/MAGI-Launcher))
- Single-instance lock — relaunching just refocuses the existing window

## Keyboard shortcuts

| Key | Action |
|---|---|
| Type | Fuzzy-filter the catalog live |
| `↑` / `↓` or `Ctrl+W` / `Ctrl+S` | Move selection up / down |
| `←` / `→` or `Ctrl+A` / `Ctrl+D` | Jump a page (10 rows, windowed list) / a column (Library grid) |
| `Enter` | Install the selected port if it isn't installed yet, launch it otherwise |
| `Shift+Enter` | Open the selected port's install folder in Explorer |
| `Alt+Enter` | Toggle fullscreen Library mode |
| `Ctrl+1`...`Ctrl+9` / `Ctrl+0` | Resize the windowed launcher to 10%...90% / 100% of screen size, windowed mode only |
| `Ctrl+-` / `Ctrl+=` | Shrink / grow the border by 1px, windowed mode only |
| `Escape` | Close the dialog on top if one is open, back out of Library mode if it's active, otherwise close the launcher |

The search box always keeps keyboard focus in the main window — every key above is intercepted there directly. Any dialog on top (Info, Settings, install progress, a file/executable picker...) gets its own keyboard focus instead: arrow keys — or the same `Ctrl+W`/`A`/`S`/`D` aliases — move the selection inside it, `Enter` activates it, `Escape` closes it (except the install-progress dialog, which can't be interrupted).

## Gamepad

Plug in an XInput controller (Xbox-style) and every window responds to it immediately, no setup needed — the same input router works on the main library and on every dialog on top of it. Moving the selection with the mouse and with the controller/keyboard always stays in sync — there's only ever one thing highlighted at a time, however you moved it there:

| Button | Action |
|---|---|
| D-pad / left stick | Move selection |
| `A` or `Start` | Install / Launch the selected port (same as `Enter`) |
| `B` | Back out of the current dialog |
| `X` | Open the Info panel for the selected port |
| `Back` | Toggle fullscreen Library mode |

## Library mode

`Alt+Enter` switches to a fullscreen grid of every port you currently have *installed* — like Steam's Big Picture library. Ports you haven't installed yet only show up in the windowed list view, never in Library mode. `Escape` (or `Alt+Enter` again) returns to the windowed view.

## Info panel

Select a port and open its **Info** panel (button, or `X` on a controller) for its installed version/tag, any setup instructions from `ports.json` (selectable, copy-pastable text), and one-click links to its website, mods page, install folder, and save folder — plus a **Save folder 2** button for ports with a second, independent save location (`save_folder2`). Any of these is simply disabled if it doesn't exist yet (not installed, the game hasn't created a save yet, or the port has no second save location at all). `↑`/`↓` (or the controller D-pad/stick) scrolls the instructions text when it's too long to fit; `←`/`→` moves between the buttons, `Enter`/`A` activates whichever one is highlighted.

Next to the version text, a **Change version** button (GitHub/GitLab ports only) fetches the last few releases and lets you install any of them instead of always the latest — handy when the newest release doesn't have a build for your platform, or you just want to roll back.

## Settings

Open it from the **◯** button in the title bar — a live, fuzzy-searchable picker over every theme in `themes.json`. Moving the selection (mouse hover, or `↑`/`↓`/the controller stick) previews a theme instantly across the whole app; `Enter`/click on it commits the choice, writing straight back to `themes.json`. Closing the dialog without confirming (`Escape`) reverts to whichever theme was actually active before you opened it — only a real selection ever gets written to disk. A row of quick-open buttons below the list — **Library**, **ports.json**, **ports.local.json**, **state.json**, **themes.json** — jumps straight to that folder/file in Explorer, greyed out if it doesn't exist yet; `←`/`→` (or the controller) moves between them, `↑`/`↓` hands focus back to the theme list.

## Useful tools

A couple of external tools come up repeatedly in `ports.json`'s **Required files** instructions, for preparing your own game data before pointing a port at it:

- **[7-Zip](https://github.com/ip7z/7zip)** — a minimal, command-line-only copy ships with Ports Launcher, but only to extract port release archives internally; get the regular graphical version separately to unpack a `.7z` (or other compressed) dump of a game you already own.
- **[extract-xiso](https://github.com/XboxDev/extract-xiso)** — not bundled, get it separately: pulls the individual files out of an Xbox/Xbox 360 `.iso`; several Xbox 360 ports' instructions explicitly ask for `extract-xiso.exe` to get their `assets` folder.

## Configuration

### `ports.json`

The main catalog, next to the executable — never bundled inside the `.exe`, so it (and the launcher itself) can be updated independently of any single port. Not meant to be hand-edited: it's replaced wholesale by catalog updates, so anything you add here yourself gets silently overwritten the next time it refreshes — see [`ports.local.json`](#portslocaljson) below to add your own ports permanently instead.

Under the hood, a save "preserved across uninstall/reinstall" (see [Features](#features) above) is a real move, not magic: uninstalling a port whose save lives inside the install folder relocates it to `Library/.saves_backup/<folder_name>/save_folder/` (or `.../save_folder2/` for the second save location) a moment before the rest of the folder is deleted, then a later install of that same port moves it straight back into place and removes the backup. It's a hidden dot-folder, only ever touched by Ports Launcher itself — worth knowing about if you're digging through `Library/` by hand for a save that seems to have vanished mid-reinstall, or if an install gets interrupted and you need to recover it manually.

### `ports.local.json`

Your own catalog, next to `ports.json` — for ports you manage yourself rather than install through Ports Launcher: you create the folder under `Library/`, put the game's files there by hand, and it shows up exactly like any other port (playable, has an Info panel, shows up in Library mode once installed). Same entry format as `ports.json`, except `source` never applies here — omit it, or set it to nothing. Since this file lives separately, replacing `ports.json` with a newer version from the maintainer never touches what you've added here.

A `folder_name` that collides with one from `ports.json` replaces the official entry with your own local one. The **Uninstall** button also behaves differently for a local port: it never deletes anything — it opens the port's folder in Explorer instead, so you stay in full control of files Ports Launcher never downloaded in the first place.

The repo ships a `ports.local.json` with two disabled example entries (`"name"`/`"folder_name"` set to `null`, which makes Ports Launcher skip them) — copy one, fill in real values, and it becomes a real entry.

### `themes.json`

```json
{
  "theme": "arc-dark",
  "font_family": "Segoe UI",
  "placeholder_text": "Type to search...",
  "show_clock": true,
  "window_size": 60,
  "border": 1,
  "themes": {
    "arc-dark": {
      "search_background": "#404552",
      "search_text": "#7c818c",
      "list_background": "#383c4a",
      "list_text": "#d3dae3",
      "selected_background": "#5294e2",
      "selected_text": "#ffffff",
      "border": "#4b5162"
    }
  }
}
```

Same format as [MAGI Launcher](https://github.com/Nyaldee/MAGI-Launcher)'s `themes.json` — switch themes live from the in-app picker (see [Settings](#settings) above), hand-editing the `theme` key here is only needed to set what the very first launch starts with. Every dialog (Info, install progress, file pickers, error/message boxes) follows the same theme too, including text-selection colors. `show_clock` toggles a clock next to the search bar (Windows' own short time format, whatever it's set to in Settings > Time & language) — defaults to `true`, set to `false` to hide it. `window_size` (0-100, a percentage of screen size) and `border` (pixels) are the windowed-mode starting values — both also live-update and persist back to this file via `Ctrl+1`...`Ctrl+0`/`Ctrl+-`/`Ctrl+=` (see [Keyboard shortcuts](#keyboard-shortcuts)), so hand-editing them is only needed to change the very first launch's size.

### `state.json`

Internal bookkeeping (installed versions, window state) that Ports Launcher writes to on its own — you shouldn't need to touch it. It also doubles as the only way to raise the GitHub/GitLab unauthenticated API rate limit for update checks: add a `"github_token"`/`"gitlab_token"` key by hand (there's no in-app field for this yet). That token is then stored in **plain text**. Never share this file, upload it anywhere, or leave it visible on a stream/screen share — unlike `ports.json`/`themes.json`/`ports.local.json`, it can contain a credential.

## Credits

Built together with [Claude](https://claude.com) (Anthropic's AI coding assistant).

## License

Copyright (C) 2026 Nyaldee. Licensed under the [GNU General Public License v3.0](LICENSE) — see the `LICENSE` file for the full text.
