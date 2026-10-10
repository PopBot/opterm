# Ordered delivery progress

The user authorized completing the remaining phases in order, committing and pushing each phase to origin/main before beginning the next. Parallel work stays within the active phase. No further design interview is queued.

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Terminal input compatibility and editable shortcuts | Complete; 38 Rust tests, strict Clippy and native X11 checks passed |
| 2 | Tab reorder, tab movement between windows, drag-out, scrollback search | Complete; 47 Rust tests, strict Clippy and focused native controls checks passed |
| 3 | Custom settings/cache location and bounded saved-output controls | Not started |
| 4 | Native platform checks and demonstrated fixes | Not started; unavailable platforms must remain unverified |
| 5 | Reproducible performance measurement, measured optimization, packaging | Not started |

## Phase 1

Editable validated app bindings apply live through the existing settings Save/Cancel/backup workflow. Duplicate bindings require explicit unbinding before reassignment. Copy-on-select defaults off. Function keys, control punctuation, modified keys, and SGR/legacy/UTF-8 terminal mouse protocols are supported. Shift bypasses program mouse reporting. Exact native clipboard text is read separately because the provisional toolkit normalizes CRLF paste events. Modern enhanced keyboard protocols, full native IME/font fallback and accessibility acceptance remain separate validation gaps.

Native checks additionally exercise shortcut reassignment with recovery-backup updates, exact paste bytes, function keys, and SGR mouse input. Existing shell PID, profile, settings and saved-layout checks pass.

## Phase 2

A compact Find row searches literal case-sensitive text in bounded live scrollback, with next/previous navigation and wraparound. It opens only on request and creates no index. App bindings include Search; unsupported older services report the limitation inside the find row without stopping work.

Tab dragging reorders tabs; dragging outside the window creates a window or targets another open window where native position information is available. A tab context menu provides explicit moves to new/existing windows. A private, bounded temporary offer contains only the tab's saved tree/name/focus and session references. The destination attaches available sessions and saves the tab before acknowledging it; the source retains its tab until acceptance and keeps it on a timeout. This is a small local file handoff, not a command ledger. Cross-window drag destination detection needs native positioning, so the explicit menu remains available on Wayland.

Native Phase 2 checks preserve all four shell PIDs while reordering, dragging a split tab into a new window and moving another tab into that window. No replacement sessions are created. Run `python3 scripts/smoke_gui.py --controls-only` for the focused check.
