# Manual cloud resume: Opterm

Prepared 2026-10-10 after the user requested ending local implementation and handing remaining work to cloud. Select repository **PopBot/opterm**, branch **codex/iterm2-everyday-features**. Verify the actual branch commit before editing. The branch contains the complete eight-item P1 implementation; delivery remains unfinished.

## Verified current state

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` pass: **126 passed, two existing opt-in tests ignored**.
- The final release's `--priorities-only` Linux GUI route passes frozen copy mode/clipboard/key isolation, readability/locator pixels, opt-in Bash prompt navigation/exact output copying, and relative links using local OSC 7 cwd.
- Earlier everyday GUI checks passed Find, semantic selection/links, switching, pane Undo, and whole-tab Undo. On the final release rerun, semantic selection, Find, switching, and rapid pane Undo pass; the whole-tab Undo exact document-equality assertion times out at `scripts/smoke_gui.py:531`. The failure screenshot visibly shows Beta restored with two panes. Diagnose actual layout/focus metadata vs fixture synchronization; do not weaken the assertion to hide a regression.
- Native Linux AT-SPI checks with actual Orca active (speech/braille disabled) pass terminal text/caret/focus/selection. Audible usability, VoiceOver and NVDA are unverified.
- The release builds with `--locked`. Final measured SHA-256: `ef6d7f3b1aa996e879649b465e18788f2b04995f4cadc98ce7239340b02af008`. See `docs/performance-current-priorities.md` and committed raw samples. Sparse default cell metadata removes 61 bytes per ordinary cell; comparisons are software-rendered, bounded and variable, not a superiority claim.
- Temporary Linux install validation passed executable/scripts/guide/icon/desktop registration. The new executable is **not installed** in the user's launcher yet. Final packaging is pending.

## Finish P1 delivery first

1. Reproduce `cargo build --release --locked` then `python3 scripts/smoke_gui.py target/release/opterm --everyday-only` on an isolated X11 display. Fix/verify the whole-tab Undo restoration assertion with preserved original shell PIDs and split/focus/name/index metadata. Rerun affected checks. Final prior GUI route was run concurrently with `--priorities-only`; determine whether concurrency/timing matters before concluding a product regression.
2. Review the final integration and legacy service capability fallbacks. Run required Rust checks and focused GUI checks after changes. Run native accessibility checks when prerequisites exist; report unavailable native checks honestly.
3. Verify local archives with `python3 scripts/package.py` and bundled Bash/zsh integration assets. Update docs with final results. Check the latest remote `main`: it advanced beyond the original `5a3b2a9` base during local work (observed `4bf9d56e53fd20df8d051a7f4b411a6d1497193b`), so preserve those changes during integration.
4. Follow AGENTS.md for detailed commits and delivery when complete. Do not treat unverified desktop behavior as release acceptance. Cloud cannot update the user's local application launcher: provide explicit local build/install commands for later, and do not claim it installed anything here.

The user's existing local session service is deliberately retained. Its handshake reports old enhanced-search/selection/shell capabilities. **Do not stop it**: stopping a service ends its sessions. Finish current work before deliberately upgrading that service; use a private `OPTERM_STATE_DIR` for new-feature trials. Neither active local shells nor subagents transfer to cloud.

## Then implement remaining nice-to-haves

Read `docs/cloud-nice-to-haves-handoff.md` and implement P2 items 9–16 in its A/B/C parallel waves: key capture/maps, searchable/tagged profiles, advanced paste, Unicode/font fidelity, named arrangements/pane movement, activity/completion alerts, optional tab appearance, and hotkey terminal window. Use exclusive file ownership and one coordinator for shared integration, with meaningful acceptance checks and complete-feature review boundaries. Evaluate P3 afterward through its bounded prototype gates.

Preserve compact terminal-first UI, exact ordinary paste and normal Tab/Ctrl+C delivery, global fonts, live session identity/control, Close-view vs End-session semantics, and bounded memory/queues. Keep SQLite, durable command ledgers, public API/CLI/MCP integration, indexed history/export, persisted clipboard history and permanent IDE/browser/provider panels deferred. The design interview is closed.

Cloud launch was **not completed**: authenticated CLI returned no configured cloud environments; browser control failed. This document is the requested manual handoff, not evidence of a running cloud job.
