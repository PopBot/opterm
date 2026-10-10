# Ordered delivery progress

The user authorized completing the remaining phases in order, committing and pushing each phase to origin/main before beginning the next. Parallel work stays within the active phase. No further design interview is queued.

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Terminal input compatibility and editable shortcuts | Complete; 38 Rust tests, strict Clippy and native X11 checks passed |
| 2 | Tab reorder, tab movement between windows, drag-out, scrollback search | Complete; 47 Rust tests, strict Clippy and focused native controls checks passed |
| 3 | Custom settings/cache location and bounded saved-output controls | Complete; 60 Rust tests, strict Clippy and native storage checks passed |
| 4 | Native platform checks and demonstrated fixes | Complete; native Linux, both Mac architectures and Windows checks passed |
| 5 | Reproducible performance measurement, measured optimization, packaging | Not started |

## Phase 1

Editable validated app bindings apply live through the existing settings Save/Cancel/backup workflow. Duplicate bindings require explicit unbinding before reassignment. Copy-on-select defaults off. Function keys, control punctuation, modified keys, and SGR/legacy/UTF-8 terminal mouse protocols are supported. Shift bypasses program mouse reporting. Exact native clipboard text is read separately because the provisional toolkit normalizes CRLF paste events. Modern enhanced keyboard protocols, full native IME/font fallback and accessibility acceptance remain separate validation gaps.

Native checks additionally exercise shortcut reassignment with recovery-backup updates, exact paste bytes, function keys, and SGR mouse input. Existing shell PID, profile, settings and saved-layout checks pass.

## Phase 2

A compact Find row searches literal case-sensitive text in bounded live scrollback, with next/previous navigation and wraparound. It opens only on request and creates no index. App bindings include Search; unsupported older services report the limitation inside the find row without stopping work.

Tab dragging reorders tabs; dragging outside the window creates a window or targets another open window where native position information is available. A tab context menu provides explicit moves to new/existing windows. A private, bounded temporary offer contains only the tab's saved tree/name/focus and session references. The destination attaches available sessions and saves the tab before acknowledging it; the source retains its tab until acceptance and keeps it on a timeout. This is a small local file handoff, not a command ledger. Cross-window drag destination detection needs native positioning, so the explicit menu remains available on Wayland.

Native Phase 2 checks preserve all four shell PIDs while reordering, dragging a split tab into a new window and moving another tab into that window. No replacement sessions are created. Run `python3 scripts/smoke_gui.py --controls-only` for the focused check.

## Phase 3

Sessions & history now includes output recording, configurable scrollback/retention/total/per-stream limits, a native folder picker, next-launch settings/cache relocation, and safe cache maintenance. Validated currently saved settings are copied without replacing a differing destination; original settings and separate last-valid recovery remain. Invalid/unavailable remembered locations report recovery/choose-location actions and block silent fallback saves. Cache cleanup is restricted to marked owned files and rejects aliases overlapping runtime or recovery.

Raw `.ansi` output uses a bounded asynchronous writer: 1 MiB queued per session with up to 512 KiB batches. Defaults are recording enabled, 30 days, 5 GiB total, 256 MiB per stream. Active files are preserved; exhaustion/failure stops capture and reports gaps without stopping PTYs. Global opt-out is applied before creating the first shell and overrides session resume. Live history is additionally capped at one million cells, excluding the capped live screen; this is not a bound on all process memory. There is no database/index or command replay.

The focused native check (`python3 scripts/smoke_storage.py`) passes recording Save/backup updates, exact ANSI output, pause/resume without backfill, staging and reopening in a custom folder, and safe clearing in both folders. Original shell PID/environment, layouts, history and independent recovery survive. Core tests also cover failed writes, retention/stream limits, overlapping paths, invalid locators and first-byte opt-out. The search test waits for final PTY output as well as child exit; the color test waits for a computed output marker rather than echoed input. Both avoid timing races. [Settings screenshot](screenshots/prototype-storage-settings-linux.png).

## Phase 4

[Native run 38030855922](https://github.com/PopBot/opterm/actions/runs/38030855922) passed all four jobs for `b30d074`: Linux x64, macOS Apple Silicon, macOS Intel and Windows x64. Unix jobs execute 71 tests with two optional fixtures explicitly ignored; Windows executes 47 applicable tests. Formatting and strict Clippy passed. Linux additionally passes full X11 desktop/storage checks and native nested Wayland rendering, input, layout saving and crash continuity.

Demonstrated fixes include accepted BSD socket blocking mode, Windows inherited Ctrl+C state, native IME cursor/preedit handling, modern monochrome emoji and installed CJK fallback, and a Wayland geometry lock deadlock. Portable byte/readiness/path fixtures preserve their exact behavior checks. Actual macOS/Windows desktop controls, OS input methods, accessibility, color/joined emoji and physical mixed-DPI monitor acceptance remain documented gaps, not claims of certification. See [native validation](platform-validation.md) and [focused desktop results](platform-native-checks.md).
