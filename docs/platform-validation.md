# Native platform validation

Phase 4 adds native build and test jobs; it does not certify every terminal program or desktop workflow. The [Native checks workflow](../.github/workflows/native-checks.yml) runs on pushes to `main`, pull requests and manual dispatch. Each job has a 20-minute limit. A newer run cancels the preceding run for the same branch.

| Native job | What it exercises |
| --- | --- |
| Ubuntu 24.04 x64 | Rust checks, native Unix PTY/socket tests, independent X11 desktop checks with software OpenGL |
| macOS 15 Apple Silicon | Native ARM build, Rust checks, Unix PTY/socket and persistence tests |
| macOS 15 Intel | Native Intel build, Rust checks, Unix PTY/socket and persistence tests |
| Windows Server 2025 x64 | Native MSVC build, Rust checks, PowerShell 7 through ConPTY and local named-pipe persistence tests |

These explicit runner labels and architectures were checked against [GitHub's hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) on 2026-10-09. PowerShell 7 is supplied by the [Windows 2025 runner image](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md). The workflow uses stable Rust with formatting and Clippy components, locked application dependencies, and a cache separated by OS, architecture, compiler version and Cargo.lock. Opterm uses file-lock APIs available since [Rust 1.89](https://doc.rust-lang.org/std/fs/struct.File.html#method.lock); a complete minimum-version compatibility test is not claimed.

All jobs run:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

`tests/native_platform.rs` supplies real native PTY/transport coverage rather than treating cross-compilation as runtime evidence. Platform-specific tests can still be skipped by `cfg` attributes; read the job's executed test list. Tests use private temporary service directories and do not connect to a user's live session service. Optional tests involving installed external agents and a preserved previous binary remain explicitly ignored unless their fixtures are supplied.

The Linux job additionally builds the desktop and runs `scripts/smoke_gui.py` and `scripts/smoke_storage.py` on separate private Xvfb displays. These cover terminal input, clipboard, settings/recovery, persistent sessions, saved layouts, and storage controls. The scripts close only their own test windows/services. Dependencies include X11/XTest, Xvfb, xwd, xclip, Mesa software rendering and DejaVu/Noto CJK fonts. Python uses its standard library.

For the same Linux checks locally, install the build/display dependencies, then run:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev libgl1-mesa-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev xvfb x11-apps libxtst6 xclip fonts-dejavu-core fonts-noto-cjk
cargo build --locked
python3 scripts/smoke_gui.py
python3 scripts/smoke_storage.py
```

## Evidence and remaining desktop checks

All four native jobs, including Linux desktop/storage/Wayland checks, passed for `b30d074` in [run 38030855922](https://github.com/PopBot/opterm/actions/runs/38030855922). Linux full desktop/storage/Wayland checks passed for `31cafca` in [run 38030527189](https://github.com/PopBot/opterm/actions/runs/38030527189); the subsequent changes only correct portable test fixtures. Follow [Opterm's Actions runs](https://github.com/PopBot/opterm/actions/workflows/native-checks.yml). A native build/test pass proves the exercised code paths on that runner, not a complete desktop acceptance pass.

Linux X11 desktop checks are automated. macOS/Windows desktop window behavior, native clipboard, folder pickers, IME composition, accessibility, mixed-DPI/monitor positioning, emoji rendering and authenticated interactive agents still need focused testing on their native desktops. Focused native Wayland rendering, keyboard input, layout saving and crash continuity now pass in a private nested compositor; see the [desktop results](platform-native-checks.md). Installing CJK outline fallback fonts improves available glyph coverage; it is not universal shaping or color-emoji support.

For a manual desktop pass, start `cargo run --locked`, create two windows with renamed tabs and nested splits, type non-ASCII text, use native copy/paste, change a launch profile/theme, stage a storage folder, and close/reopen the desktop. Verify preserved shell state/PIDs and layout, then exercise settings recovery with the original files retained. Shut down the isolated test service only after its test work is finished. Keep these results separate from automated PTY/transport tests and from future speed measurements.

## Demonstrated fixes in this phase

Installed CJK outline fonts are now registered as a terminal fallback; a glyph test verifies the loaded face when present. Native IME is enabled at the controlled terminal cursor, with a bounded local preedit preview. Only a final UTF-8 commit becomes PTY input; cancelled compositions do not follow focus into another shell. Unit coverage checks these rules, while actual OS input-method acceptance remains pending.

A private native Wayland run exposed a context-lock deadlock when saving window geometry: unlike X11, Wayland omits absolute native window coordinates. The viewport fallback is now read before entering the context's input lock. The focused native check verifies layout creation and preserved shell state after the fix.

The initial native matrix also exposed Rust 1.99's deprecation of the old atomic queue reservation API. A portable compare-exchange loop retains the queue bound and supports both the local compiler and the hosted stable compiler.

macOS native tests exposed accepted sockets inheriting the nonblocking listener flag. Accepted peer streams now explicitly block between framed requests, with an idle-after-Hello regression check.

The first Windows run passed ConPTY resize/colors/Unicode and named-pipe reattachment, but failed Ctrl+C interruption. The correction clears an inherited ignore flag where a console is attached and removes the unnecessary new-process-group flag from detached service launch. Microsoft documents the behavior in [process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) and [SetConsoleCtrlHandler](https://learn.microsoft.com/en-us/windows/console/setconsolectrlhandler). The native test requires the foreground command's interruption marker rather than assuming PowerShell must exit, including through the detached service. The Windows native job passed these checks in [run 38030527189](https://github.com/PopBot/opterm/actions/runs/38030527189).

An older terminal-status fixture now compares the exact six expected reply bytes while ignoring the different formatting whitespace emitted by GNU and BSD `od`. Both native Mac jobs had already passed the new PTY/transport tests before reaching that fixture.

Native Mac testing also exposed readiness markers matching initial command echo before the shell created its output files. The fixtures now require a marker generated by execution. Profile working directories compare canonical filesystem identity across macOS `/var` and `/private/var` aliases. PID, environment, exact terminal replies and persistence assertions remain intact. Both Mac architectures subsequently passed every executed test target.
