# Ordered delivery progress

The user authorized completing the remaining phases in order, committing and pushing each phase to origin/main before beginning the next. Parallel work stays within the active phase. No further design interview is queued.

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Terminal input compatibility and editable shortcuts | Complete; 38 Rust tests, strict Clippy and native X11 checks passed |
| 2 | Tab reorder, tab movement between windows, drag-out, scrollback search | Not started |
| 3 | Custom settings/cache location and bounded saved-output controls | Not started |
| 4 | Native platform checks and demonstrated fixes | Not started; unavailable platforms must remain unverified |
| 5 | Reproducible performance measurement, measured optimization, packaging | Not started |

## Phase 1

Editable validated app bindings apply live through the existing settings Save/Cancel/backup workflow. Duplicate bindings require explicit unbinding before reassignment. Copy-on-select defaults off. Function keys, control punctuation, modified keys, and SGR/legacy/UTF-8 terminal mouse protocols are supported. Shift bypasses program mouse reporting. Exact native clipboard text is read separately because the provisional toolkit normalizes CRLF paste events. Modern enhanced keyboard protocols, full native IME/font fallback and accessibility acceptance remain separate validation gaps.

Native checks additionally exercise shortcut reassignment with recovery-backup updates, exact paste bytes, function keys, and SGR mouse input. Existing shell PID, profile, settings and saved-layout checks pass.
