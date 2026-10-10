# Opterm

A lightweight cross-platform terminal for macOS, Linux, and Windows, with built-in session management for everyday shells and existing AI CLI agents.

## Current status

The Rust prototype runs a real local shell in the approved thin layout, with tabs, splits, multiple windows, resize, selection, copy/paste, and scrollback. A small background service now keeps shells and terminal programs alive when you close or crash the desktop. Reopening Opterm reconnects to those live sessions. Settings now include appearance, launch profiles, per-profile terminal themes, TOML configuration, and a last-valid recovery backup. No database is involved. See [prototype status and limitations](docs/prototype-status.md).

## Run locally

```sh
cd /home/jarod/Dev/extdev/projects/opterm
cargo run --release
```

After the first build, launch directly:

```sh
./target/release/opterm
```

Close the window and run the same command again to return to your shells. Start an installed agent from a shell as usual (`codex` or `claude`), or create a new agent session directly:

```sh
./target/release/opterm -- codex
```

An explicit program creates a new session; reopening with no program restores existing sessions. The **Sessions** menu reattaches detached tabs. Cmd+W (macOS) or Ctrl+Shift+W (Linux/Windows) detaches the focused pane (the tab closes when its last pane is detached). **End session…** in the menu stops its work after confirmation. Use **New window** to open a fresh shell. Launching another ordinary instance restores existing sessions; controlled sessions are read-only there until you choose **Take control**.

Splits keep the thin layout, with draggable dividers and a subtle outline around the focused pane. Each new pane starts a separate session using the default profile. Click a pane or use the focus shortcuts below; keyboard input and copy/paste use the focused pane. Closing a pane collapses its split and detaches its session. **Detach tab** in the menu detaches all its panes.

| Action | Linux / Windows | macOS |
| --- | --- | --- |
| Settings | Ctrl+Shift+, | Cmd+, |
| New window | Ctrl+Shift+N | Cmd+N |
| Split right | Ctrl+Shift+E | Cmd+D |
| Split below | Ctrl+Shift+O | Cmd+Shift+D |
| Focus another pane | Ctrl+Shift+Arrow | Cmd+Alt+Arrow |
| Resize nearest divider | Ctrl+Shift+Alt+Arrow | Cmd+Alt+Shift+Arrow |
| Maximize / restore pane | Ctrl+Shift+Enter | Cmd+Shift+Enter |
| Detach focused pane | Ctrl+Shift+W | Cmd+W |

Terminal output supports ANSI colors, the 256-color palette, and 24-bit RGB. New sessions advertise `TERM=xterm-256color` / `COLORTERM=truecolor` and remove an inherited `NO_COLOR` flag from development launchers. Programs choose their text colors; plain shell output remains plain. Existing shells keep their environment: run `unset NO_COLOR` (POSIX shells) to enable color for subsequently launched programs. Existing programs may need their own color setting or a normal restart; Opterm does not restart them.

Open **… → Settings…** to adjust app appearance, monospace font, size, padding, line height, and cursor. **Save** applies changes without restarting shells; **Cancel** discards the draft. Invalid TOML edits keep your working settings and good recovery copy. **Restore last valid settings** loads the backup for review; click **Save** to restore it. See [settings locations and recovery behavior](docs/settings-foundation.md).

In **Settings → Profiles**, create shell/agent presets with executable, arguments, starting folder, environment, and a terminal theme. Choose a default, right-click **+** to select a profile, or use the profile menus for tabs/splits. Neutral Dark/Light themes and custom colors are supported. Saving a profile affects new launches; editing its chosen theme updates live views without restarting programs. Marked secret environment values are omitted from the backup and must be reentered after recovery. See [profiles and themes](docs/profiles-themes.md).

Live work survives window closure, but split layouts are currently window-local: reopening restores sessions as individual tabs. Saved split/window layouts are a later step.

The service stays running when all windows close. Rebuilding the app leaves an existing service running its current code to preserve work. Older services remain usable but reject the new profile/theme features until deliberately upgraded after work finishes. After finishing its sessions, deliberately stop it to load the rebuilt service on the next launch. To deliberately stop it and end **all** its sessions:

```sh
./target/release/opterm --stop-service
```

Use an installed Rust toolchain and a native desktop session. The first build downloads dependencies. Linux X11 is tested; macOS, Windows, and Wayland runtime behavior remains unverified. Reboot, logout, or service failure can end work; this prototype does not replay old commands or restore lost processes.

## Project documents

- [Product brief](docs/product-brief.md)
- [UI, settings, and first-version specification](docs/ui-settings-spec.md)
- [Terminal-first prototype plan](docs/prototype-plan.md)
- [Decision history](docs/design.md)
- [Approved layouts and historical mockups](docs/design/mockups/README.md)
- [Glossary](GLOSSARY.md)

## Implementation direction

Use Rust for the shared core and independent background session service. Follow the accepted thin terminal layout and implement standard windows, tabs, splits, profiles, themes, shortcuts, settings, and detach/reattach workflows. Evaluate the UI toolkit through a working terminal slice and native platform checks.

Start with simple configuration, layout, and bounded history files. SQLite, a durable direct-command ledger, public execution API/JSON CLI/MCP integration, and indexed history/export are deferred from the first usable version.

Live sessions can survive closing or crashing desktop views while the background service remains alive. This does not promise arbitrary process survival through reboot or service failure, and commands must not be automatically replayed.

Opterm uses independent styling. Historical other-product design references are retained only as design records and must not become application dependencies or shipping assets.
