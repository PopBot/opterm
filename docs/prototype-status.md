# Working terminal slice — 2026-10-09

This is the first internal development slice, not the complete first usable version defined by Q54.

## What runs

- A native desktop window with a 30-pixel tab row and a continuous charcoal terminal surface; native title-bar decorations are supplied by the OS/window manager.
- A real local PTY and shell, or an executable and argument list supplied on the command line.
- ANSI colors, cursor positioning, terminal-query replies, alternate-screen programs, resize, and bounded 10,000-line scrollback.
- Multiple shell tabs, automatic program titles, manual naming (double-click a tab or use the menu), pointer text selection, copy/paste, and mouse-wheel scrollback.
- New-tab shortcut: Cmd+T on macOS, Ctrl+Shift+T on Linux/Windows. Copy/paste: Cmd+C/V or Ctrl+Shift+C/V. Ordinary Ctrl+C reaches the shell interrupt.
- The accepted 14-pixel font, 6-pixel terminal inset, and 1.2 line spacing, using an installed monospace font with a bundled fallback.

Output parsing runs on a PTY reader thread and wakes the UI when output arrives. The renderer draws the active viewport; inactive tabs continue parsing but do not generate terminal draw calls. No database or transcript index is introduced.

## Provisional stack

Rust, portable-pty 0.9.0, alacritty_terminal 0.26.0, and egui/eframe 0.33.3 (OpenGL). The manifest and Cargo.lock record the dependency selection. egui/eframe is an additional provisional renderer candidate chosen to get a native Rust terminal slice running; this is not a final toolkit selection or evidence of superiority over Slint/GPUI. It uses native window/input integration and no webview.

Project-local Codex configuration specifies `gpt-6.1-sol` and `high` for subsequent runs. A configuration file cannot switch an already-running response.

## Run and verify

From the repository root:

```sh
cargo run --release
# Or launch an existing CLI agent as the terminal program:
cargo run -- -- codex
cargo run -- -- claude

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
# Optional local agent CLI help-launch check:
cargo test --test pty existing_agent_clis_start_in_native_pty -- --ignored
# Linux X11 check on a private display; needs Xvfb, xwd, libX11/libXtst:
cargo build
python3 scripts/smoke_gui.py
```

The GUI check sends real native key events, creates another tab, interrupts `sleep`, resizes the window, runs a shell command, round-trips a pointer selection through the native clipboard, and captures `target/prototype-linux.png`. This isolated X11 environment has no window manager, so the screenshot does not show the OS title bar. A check of shell output is not a benchmark or a complete agent/TUI acceptance test.

[Actual Linux prototype screenshot](screenshots/prototype-linux.png), captured from the running release binary.

## Current limits and next slice

**Closing this prototype ends its shells.** Session ownership is currently inside the desktop process. Independent service ownership and detach/reattach are the next slice; the accepted persistence requirement remains unchanged. Tab termination/closing is not exposed as a detach operation.

Multiple windows, splits, profiles, settings/TOML/recovery backup, saved history, search, configurable shortcuts, theme editing, and layout restoration are not implemented yet. The terminal core supports bracketed paste without appending Enter; the provisional UI integration normalizes Windows-style clipboard line endings, which must be corrected before accepting Q83 in the complete product. Mouse reporting, advanced keyboard protocols, IME composition, full Unicode/font fallback, accessibility, and remaining key bindings require further work. Ordinary typing and supported key sequences are not a claim of universal TUI compatibility.

Validation on 2026-10-09: formatting and strict Clippy passed; six regular tests passed, plus the explicitly run optional Codex/Claude help-launch test. Debug and release builds passed the isolated Linux X11 GUI check. The optimized Linux executable is available at `target/release/opterm`. Native Linux PTY and GUI checks are recorded for this slice. Linux Wayland, macOS, Windows, authenticated interactive CLI-agent behavior beyond help launches, and comparative speed/resource benchmarks remain unverified. Native runtime libraries/display access are required for GUI launch; cross-platform dependencies alone do not prove native coverage.

Move session ownership into the small independent service next, then add detach/reattach through the compact picker. Continue the accepted terminal controls and settings after continuity works, keeping the backend scope simple.
