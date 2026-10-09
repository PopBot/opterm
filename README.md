# Opterm

A lightweight cross-platform terminal for macOS, Linux, and Windows, with built-in session management for everyday shells and existing AI CLI agents.

## Current status

The Rust prototype runs a real local shell in the approved thin layout, with tabs, resize, selection, copy/paste, and scrollback. A small background service now keeps shells and terminal programs alive when you close or crash the desktop. Reopening Opterm reconnects to those live sessions. No database is involved. See [prototype status and limitations](docs/prototype-status.md).

## Run locally

```sh
cd /home/jarod/Dev/extdev/projects/optimized-terminal
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

An explicit program creates a new session; reopening with no program restores existing sessions. The **Sessions** menu reattaches detached tabs. Cmd+W (macOS) or Ctrl+Shift+W (Linux/Windows) detaches a tab. **End session…** in the menu stops its work after confirmation. Launching another instance opens another window; existing controlled sessions are read-only there until you choose **Take control**.

The service stays running when all windows close. To deliberately stop it and end **all** its sessions:

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
