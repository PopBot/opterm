# Terminal, persistent-session, split/window, settings, and profile/theme slices — 2026-10-09

This is the first internal development slice, not the complete first usable version defined by Q54.

## What runs

- A native desktop window with a 30-pixel tab row and a continuous charcoal terminal surface; native title-bar decorations are supplied by the OS/window manager.
- A real local PTY and shell, or an executable and argument list supplied on the command line.
- ANSI 16/256-color and 24-bit RGB foreground/background, cursor positioning, terminal-query replies, alternate-screen programs, resize, and bounded 10,000-line scrollback.
- Multiple shell tabs and independent native windows, nested splits in both orientations, draggable/keyboard-resizable dividers, mouse/keyboard pane focus, temporary pane maximization, automatic program titles, manual naming (double-click a tab or use the menu), pointer text selection, copy/paste, and mouse-wheel scrollback.
- New-tab shortcut: Cmd+T on macOS, Ctrl+Shift+T on Linux/Windows. Copy/paste: Cmd+C/V or Ctrl+Shift+C/V. Ordinary Ctrl+C reaches the shell interrupt.
- Independent background PTY/VT ownership, automatic live-session restoration on reopening, a compact Sessions picker, detach, explicit End session confirmation, and one input/resize controller with deliberate takeover. Manual names survive desktop closure while the service is alive.
- A compact separate settings window with editable appearance, Save/Cancel, validated TOML, live application, external-edit conflict handling, and a last-valid recovery copy. See [settings foundation](settings-foundation.md). The Profiles section also supports compact launch-preset and per-profile color-theme editors; other sections identify their pending controls.
- The accepted 14-pixel font, 6-pixel terminal inset, and 1.2 line spacing, using an installed monospace font with a bundled fallback.

Output parsing and bounded scrollback stay in the service, including when no desktop is attached. The UI batches adjacent keystrokes into bounded packets while retaining event order across focus/paste/control changes, and uses a background IPC worker and cached screen snapshots; socket reads and PTY writes stay off the rendering thread. It polls the visible pane viewports at about 16 ms and the compact session list every 500 ms. Unchanged screens are skipped. This is a simple full-snapshot protocol, not the final throughput optimization or a benchmark result. Inactive sessions continue parsing without terminal draw calls. No database or transcript index is introduced.

New service-owned sessions advertise truecolor and discard an inherited NO_COLOR flag so development tooling does not suppress normal interactive program colors. Existing sessions/processes retain their environments; no running work is restarted to apply this change.

The same executable starts its service on demand. A process lock prevents duplicate services from replacing live work. Transport uses a private Unix socket on Linux/macOS and an owner-restricted Windows named pipe, with versioned length-framed JSON. The prototype bounds messages to 32 MiB, desktop queued input to 1 MiB, PTY input packets to 1 MiB with a 64-packet queue, and sessions to 128. These are implementation limits pending measurement, not accepted performance budgets. Each session has its own writer thread so a blocked program cannot block desktop rendering or terminal-query parsing. Rejected input is reported, never retried automatically.

The service holds current screen state and 10,000 scrollback lines in memory. It does not save terminal history to disk. Closing a window, detaching a tab, or a desktop crash releases that view/control but keeps its processes running. End session removes that session and stops its child; intentionally daemonized descendants are not tracked as a job tree. Exited sessions remain inspectable until ended. A second desktop can observe existing work; input, resize, selection, and scroll currently require control. Observer-local scroll/selection is still future work.


Split geometry and focused-pane state belong to the desktop window; the service protocol/PTY ownership remains unchanged. Every split launches a separate session using the default profile, with profile-selection menus for explicit choices. Visible panes receive screen updates independently, and keyboard input/copy/paste goes to the focused pane; wheel scrolling targets the hovered pane. Thin dividers have a 5-pixel drag target, with an 80-pixel minimum pane extent when available. Closing a pane collapses its branch and detaches its session; Detach tab detaches all panes. Temporarily maximizing a pane preserves its previous tree/ratios; pane focus shortcuts cycle through hidden panes while maximized.

New Window launches another instance with `--new-window`, creating a fresh default-profile session in its own native window. It shares the same session service and does not attach or take control of other windows' sessions. Closing the originating desktop leaves the new window usable. Ordinary launches still restore existing sessions. Window-specific layouts are now saved; reopening restores their tabs, split geometry, names, active tab, and focused pane while reconnecting surviving sessions. Profiles now capture executable/arguments, environment, starting folder, and theme ID. New panes use profile folder → workspace folder → home; invalid configured folders report an error. Manual tab names stay with a split tab during pane closure and are included in saved layouts. [Current shortcuts and local run commands](../README.md).

Runtime files default to the platform application-data folder, under `opterm/runtime` (Linux: `$XDG_DATA_HOME/opterm/runtime`, usually `~/.local/share/opterm/runtime`; macOS: `~/Library/Application Support/opterm/runtime`; Windows: `%LOCALAPPDATA%\opterm\data\runtime`). The directory holds the service endpoint/lock and diagnostic startup log. `OPTERM_STATE_DIR` can override it for isolated development runs; every instance must use the same override to reconnect. Unix directories must be user-owned with mode 0700. Settings and recovery now use separate preferences/application-data locations; the folder picker and cache controls remain ahead. See [exact settings paths](settings-foundation.md). Changing this override selects a different service, so keep it stable for live work.

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

The GUI check uses private service, settings, and recovery directories and sends real native key events, creates another tab, interrupts `sleep`, resizes the window, runs a shell command, and round-trips a pointer selection through the native clipboard. It closes the window through WM_DELETE_WINDOW, reopens, force-kills the desktop, and reopens again. Both cycles check the same shell PID, environment variable, and continuing background work. It also checks the separate settings window, Save/Cancel, live font resizing without changing the shell PID, external edit conflicts, corrupt settings recovery, and failed Save/retry. The check then exercises nested splits, separate PIDs/variables, focus, resizing, maximize/restore, pane detach, and New Window. It captures `target/prototype-linux.png` and deliberately stops only its own test service afterward. This isolated X11 environment has no window manager, so the screenshot does not show the OS title bar. A check of shell output is not a benchmark or a complete agent/TUI acceptance test.

[Profile editor](screenshots/prototype-profiles-linux.png), [theme editor](screenshots/prototype-themes-linux.png), [Light terminal](screenshots/prototype-light-linux.png), [settings screenshot](screenshots/prototype-settings-linux.png), [actual Linux prototype screenshot](screenshots/prototype-linux.png) and [split layout screenshot](screenshots/prototype-splits-linux.png), and [restored windows screenshot](screenshots/prototype-layouts-linux.png), captured from the running release binary.

## Current limits and next slice

**Live sessions survive closing or crashing the desktop while the independent service remains alive.** Reboot, logout, service failure, or intentional service shutdown can end work. There is no process resurrection or automatic command replay. A service disconnect is reported; reopen the app to reconnect. Automatic reconnection and saved ended/unknown metadata are still future work.

Tab reordering/movement between windows, custom storage/cache controls, saved history, search, and configurable shortcuts are not implemented yet. [Saved window/tab/split layouts](saved-layouts.md) now restore arrangements using service/session references without replaying commands. The terminal core supports bracketed paste without appending Enter; the provisional UI integration normalizes Windows-style clipboard line endings, which must be corrected before accepting Q83 in the complete product. Mouse reporting, advanced keyboard protocols, IME composition, full Unicode/font fallback, accessibility, and remaining key bindings require further work. Ordinary typing and supported key sequences are not a claim of universal TUI compatibility.

Validation on 2026-10-09: formatting and strict Clippy passed; thirty-two regular tests passed, including settings/profile recovery, secret exclusion, migration, theme import/export, real launch configuration, and theme-aware color-query tests. The optional prior-service compatibility check also passed against the preserved preceding release. Real service tests cover color-environment normalization plus basic/bright/indexed/RGB colors through IPC, disconnect/detach continuity, retained names and scrollback, output while detached, an alternate-screen foreground agent fixture with retained process state, controller rejection/takeover, duplicate-service startup, and no command replay after service failure. The isolated Linux X11 GUI check passed, including profile creation/default launch, Light/custom themes, live colors without process replacement, default-profile splits, safe profile/theme deletion, and settings Save/Cancel, live font changes with the same shell PID, external conflict/replace, corrupt-file recovery, and failed Save/retry, alongside normal close and desktop SIGKILL continuity, nested split input isolation, pointer/keyboard focus including rapid text-then-click ordering, drag/keyboard resizing, maximize/restore, detach/collapse without ending work, and a new window remaining usable after the original window closes. The saved-layout slice also verifies two-window reopening with retained PIDs/environment, manual tab names, split/focus restoration, and unavailable panes with zero replacement sessions after service restart. The optimized Linux executable is available at `target/release/opterm`. The earlier Codex/Claude help-launch check passed again after the service change; authenticated interactive agent behavior remains unverified. A deterministic agent fixture is not a full provider-agent acceptance test.

Linux Wayland, macOS, Windows, comparative speed/resource benchmarks, and remaining terminal compatibility cases remain unverified. Native runtime libraries/display access are required; cross-platform dependencies alone do not prove native coverage. Saved layouts now have focused Linux X11 coverage. Remaining everyday controls, terminal compatibility, and native platform validation are ahead; keep the backend scope simple. See [profiles and themes](profiles-themes.md) for migration and service-upgrade details.
