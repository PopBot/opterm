# Focused native desktop results

Checked on 2026-10-09 on Linux Mint 22.3, x86-64. These are bounded prototype checks, not complete platform acceptance. The separate [native validation plan](platform-validation.md) describes the hosted build/test matrix and remaining desktop cases.

| Surface | Observed result | Limits |
| --- | --- | --- |
| Linux X11, private Xvfb | Native input, exact CRLF clipboard paste, function/modified keys, terminal mouse reports, settings/recovery, profiles/themes, saved layouts and original-shell continuity pass | Does not establish every shell/TUI, IME or accessibility behavior |
| Linux X11 everyday controls | Literal scrollback search, tab order, drag into a new window and explicit move into another window preserve names, split trees, focus and all four original shell PIDs | Mixed-DPI and changed monitor arrangements remain untested |
| Linux X11 storage | Native recording settings, raw ANSI bytes, per-session pause/resume without backfill, staged settings/cache relocation, backup independence and safe cache clearing pass | Desktop folder-picker interaction remains untested |
| Linux native Wayland, nested Weston 13.0.0 | Rendering and ASCII keyboard input pass; actual terminal cells retain CJK width and combining text; saved-layout files are created; desktop SIGKILL/reopen preserves original shell PID/environment; a native settings window opens | Nested compositor uses a software renderer; it is not a physical Wayland desktop/DPI/GPU test |
| macOS / Windows desktops | Not exercised from this Linux machine | Native hosted PTY/transport/build tests have their own workflow evidence; they do not certify desktop workflows |

## Wayland reproduction

`scripts/smoke_wayland.py` launches its own nested Weston compositor on a private Xvfb display. Opterm has `DISPLAY` removed, explicitly selects Wayland and connects to a private Wayland socket. This prevents an accidental X11 fallback from being counted as Wayland evidence. Service, settings and recovery paths are private temporary directories. The test stops only its own compositor and session service.

Use an installed Weston, or extract its matching Ubuntu `weston` and `libweston-13-0` packages into `target/wayland-tools/root` without installing them system-wide. The script maps their modules and shared libraries locally. On the tested machine the remaining backend dependencies were already installed.

```sh
cargo build --locked
python3 scripts/smoke_wayland.py
```

The passing local run produced `target/wayland-gui.log`, `target/wayland-app.log`, `target/wayland-compositor.log`, `target/prototype-wayland-linux.png` and `target/prototype-wayland-settings-linux.png`. The application protocol log confirms native Wayland surface/input traffic. Screenshots show the compact terminal and settings window; passing assertions additionally verify the original live shell and saved layout creation.

The first native run exposed a genuine Wayland-only deadlock: saved geometry called the context viewport fallback while already inside its input lock. X11 normally supplied the geometry directly and skipped that branch. Moving the fallback read outside that lock resolved the panic, and the focused native run passed afterward.

## Unicode, input methods and accessibility

The visible fixture is `ASCII | café é | 中文 日本語 | 👩‍💻 🙂`. Installed Noto CJK outline fallback now renders the tested Chinese/Japanese characters; the service snapshot separately verifies wide CJK cells and combining text. This sample does not prove universal script shaping or complete grapheme behavior.

Color emoji and joined emoji sequences require separate acceptance; missing glyphs were observed in the initial Wayland sample. Native IME/preedit implementation and its unit checks do not substitute for an actual IBus/Fcitx/macOS/Windows composition session. Native Wayland clipboard, normal window-manager close, screen-reader/AT-SPI access to the terminal grid, mixed-DPI monitors and macOS/Windows desktop controls remain explicitly unverified. X11 normal close and crash continuity were already exercised separately.
