# Cloud handoff: remaining iTerm2-inspired improvements

Prepared 2026-10-10. This is a plan and a copyable kickoff prompt; no cloud task has been started. The user requested handing remaining work to cloud. Start with [the manual cloud resume](cloud-resume.md) on the pushed codex/iterm2-everyday-features branch, finish P1 verification/delivery, then proceed to the P2/P3 waves below. Record the actual base SHA.

The feature source is [P2/P3 in the roadmap](iterm2-roadmap.md). Read [the accepted settings specification](ui-settings-spec.md), [the active prototype sequence](prototype-plan.md), and [current implementation evidence](prototype-status.md) first. Later accepted corrections override historical plans. The design interview is closed.

## Prepare the cloud environment

Select the Opterm repository in a cloud environment, have setup install and test its dependencies, review the setup report, and publish the environment before starting implementation. Each cloud task has its own workspace, so separate tasks need an explicit shared base commit and integration plan. These workflow details were checked against [OpenAI's Codex Cloud guide](https://learn.chatgpt.com/docs/cloud) on 2026-10-10.

Use the repository's [native-checks workflow](../.github/workflows/native-checks.yml) as the dependency source. Request Rust with rustfmt/clippy, Python 3.11 or newer, the listed Linux graphics/window-system development libraries, Xvfb, xwd, libX11/libXtst, xclip, and the fixture fonts. Test `cargo build --locked` during setup. Keep the environment's reusable installation separate from task-specific private test services. The current environment supports tested installation commands and startup instructions; review and republish when its reusable setup changes. See [OpenAI's cloud environment documentation](https://learn.chatgpt.com/docs/environments/cloud-environments).

A Linux cloud workspace can run Rust and private-display checks. It cannot establish macOS/Windows interaction, audible screen-reader usability, desktop-compositor hotkeys, or hardware-GPU performance. Keep those evidence gaps visible and provide native check instructions for the corresponding hosts.

## Parallel ownership and PR boundaries

Use one coordinator and up to three independent workers per wave. Workers use isolated `codex/` branches/worktrees or separate cloud-task workspaces from the same recorded base. Share API contracts before wiring them. If subagents are unavailable in a cloud surface, use separate feature tasks or implement the same sequence serially; never assume a product surface exposes a particular delegation tool.

The coordinator exclusively owns existing shared integration files: `src/main.rs`, `src/input.rs`, `src/settings.rs`, `src/settings_ui.rs`, `src/shortcuts.rs`, `src/profiles.rs`, `src/workspace.rs`, `src/terminal.rs`, `src/service.rs`, `src/service/client.rs`, `src/lib.rs`, `Cargo.toml`, `Cargo.lock`, shared GUI smoke scripts, and shared status/user-guide docs. Inspect the actual module layout before assigning ownership. An existing file not listed here must also have exactly one named owner.

Workers own their new feature module, its dedicated tests, and one feature-specific acceptance document. Names in the table are proposed; confirm they do not exist before creating them. A worker that needs an existing integration file sends the coordinator a patch/API proposal or receives an explicit exclusive handoff first. This rule also applies to Cargo changes, protocol fields, and settings migrations.

Each feature gets one focused PR with its complete behavior, integration, compatibility notes, and evidence. A helper-only worker branch is an intermediate handoff, not a completed feature PR. The coordinator incorporates workers sequentially, fixes overlap, runs checks, and opens the complete feature PR against the current integration base. Do not combine unrelated P2 features in one review. Later waves rebase on the tested preceding wave. Follow the current AGENTS.md commit/merge/push instructions through one coordinator; workers never race to update `main`. Publishing a release remains outside this handoff.

## P2 implementation waves

| Wave / feature PR | Exclusive worker files | Coordinator integration and dependencies |
| --- | --- | --- |
| A1 — key capture and maps, `codex/p2-key-maps` | New `src/key_maps.rs`, `tests/key_maps.rs`, `docs/acceptance/key-maps.md` | Capture UI, shortcut conflicts, profile terminal maps, input dispatch and additive migration. Separate app actions from terminal byte mappings. |
| A2 — searchable/tagged profiles, `codex/p2-profile-catalog` | New `src/profile_catalog.rs`, `tests/profile_catalog.rs`, `docs/acceptance/profile-catalog.md` | Profile persistence, launcher/settings filters, favorites/tags, explicit current-profile launch actions. No renderer or service dependency. |
| A3 — advanced paste, `codex/p2-advanced-paste` | New `src/paste_policy.rs`, `tests/paste_policy.rs`, `docs/acceptance/advanced-paste.md` | Compact preview/edit UI and bounded paced-input queue. Existing ordinary/bracketed paste is the baseline. Use current controller/session identity at dispatch. |
| B1 — Unicode/font fidelity, `codex/p2-text-fidelity` | New `src/text_layout.rs`, `tests/text_fidelity.rs`, `docs/acceptance/text-fidelity.md`, dedicated fixture assets | Coordinator owns renderer changes and font/glyph caches. Establish text/cell geometry contracts with selection, search, links, copy mode, and accessibility before implementation. |
| B2 — named arrangements/pane movement, `codex/p2-arrangements` | New `src/arrangements.rs`, `tests/arrangements.rs`, `docs/acceptance/arrangements.md` | Existing layout persistence, pane menus/dragging and cross-window service transfers. Depends on current service ownership and Undo-close invariants. |
| B3 — activity/completion alerts, `codex/p2-activity-alerts` | New `src/activity.rs`, `tests/activity.rs`, `docs/acceptance/activity-alerts.md` | Bounded/coalesced service events, small tab indicators, native notifications and opt-in settings. Completion requires P1 shell markers, not inferred prompt text. |
| C1 — optional tab appearance, `codex/p2-tab-appearance` | New `src/tab_appearance.rs`, `tests/tab_appearance.rs`, `docs/acceptance/tab-appearance.md` | Coordinator integrates widths, visibility, subtle profile colors and optional orientation. Build on B's pane movement; preserve compact top-row defaults. |
| C2 — hotkey terminal, `codex/p2-hotkey-window` | New `src/hotkey_window.rs`, `src/platform_hotkey/` platform files, `tests/hotkey_window.rs`, `docs/acceptance/hotkey-window.md` | Native shortcut registration and dedicated window lifecycle. Platform workers may each own only their OS file; coordinator owns module registration/dependencies and shared window behavior. Depends on live service reconnection. |

Within A and B, helpers and dedicated tests run in parallel; integration into shared files is serial. In C, native hotkey adapters can proceed alongside tab policy. Do not let optional visual work delay compatibility fixes found in A/B. Automatic host/cwd theme switching can be a separate profile-catalog follow-up after OSC metadata and user override precedence are tested. Modern keyboard reporting and ligatures are gated follow-ups, not shortcuts around the tests below.

## Concrete P2 acceptance

| Feature | Required evidence before calling its slice complete |
| --- | --- |
| Key capture/maps | Equal-width binding controls remain aligned; capture/cancel/save works without forwarding captured keys. Conflicts are shown before Save. Old files and explicitly disabled/custom bindings survive migration. Real PTY byte capture proves ordinary Tab completion, Shift+Tab, Ctrl+C interrupt, configured Alt/Option behavior, and bracketed paste. Any modern keyboard protocol must negotiate on/off and pass sequence/conformance fixtures plus editor/pager checks. |
| Profiles | Case-insensitive filtering of name/tags, favorites ordering, stable profile identity through rename, and atomic persistence/reload. New tab/split with current profile is explicit; ordinary new-tab/default-profile behavior stays intact. Editing a theme/profile never restarts an existing shell or retroactively changes launch environment/cwd. |
| Paste | Exact UTF-8/newline bytes from native clipboard reach the PTY with no added Enter. Preview Cancel sends zero bytes; Edit sends only approved text. Bracketed paste delimiters appear exactly once when negotiated. Pacing has bounded bytes/queue and cancellation; stale or lost control stops delivery instead of redirecting it. Transformations are explicit and multiline confirmation defaults off. |
| Text fidelity | Native screenshots and cell/selection assertions for combining accents, emoji variation selectors/ZWJ, CJK/wide cells, fallback, bold/italic, and 1x/2x/fractional scale. Search highlights, clickable-link regions, copy text and accessibility text agree with cell positions. No clipping after resize or terminal wrap. Compare matched release rendering/latency/RSS before and after; ligatures stay disabled until alignment and resource evidence pass. Fonts remain global. |
| Arrangements/movement | Save/rename/delete/reload named presets; reconnecting a live layout preserves IDs/PIDs, manual titles, splits and focus. Duplicate explicitly creates new sessions only on request. Moves and their menu equivalents preserve live PIDs/control; canceled/failed cross-window transfers roll back without orphaning or double-control. Undo close still restores references. End session remains distinct from closing a view. |
| Alerts | Defaults silent. Each enabled channel obeys mute/rate limits and excludes unwanted foreground/self-generated noise. Completion is tied to shell markers; missing integration shows its limitation. A sustained output flood has bounded event memory/CPU and no notification storm. Native notification click focuses the correct still-live session, including detached/other-window cases, without silently taking control. |
| Tab appearance | Defaults match the current compact top row. Many/long titles, close buttons, overflow, active-tab visibility, context menus, reordering, snap targets and keyboard navigation stay usable at narrow widths and display scales. Optional left tabs remain a tab strip. No hidden close/menu targets or clipping; label and tab-color contrast are tested. |
| Hotkey window | Repeated show/hide preserves session IDs/PIDs, focus and window placement across monitors/scales. Registration collision/denial gives a usable fallback; unregister on shutdown and recover after restart. Test native macOS, Windows, X11 and supported Wayland compositor/portal paths separately. Unsupported paths have an explicit normal-launcher fallback. No global shortcut claim based solely on a Linux build. |

Every PR also checks detached/observer views remain read-only, terminal input targets only the controlled focused session, settings changes preserve running processes, and bounded caches/queues have deterministic limit tests. Choose focused checks appropriate to the feature rather than adding tests that merely restate its implementation.

## P3 evaluation after compatibility and native validation

These remain evaluations. Produce one isolated prototype/design PR per item, with an explicit promotion decision and measured budgets before integrating it into the default runtime. Complete the affected P2 compatibility work and document the remaining native coverage first.

| Evaluation | Dependency and bounded acceptance |
| --- | --- |
| Instant replay | After B text fidelity. Use a per-session byte/frame/time cap, deterministic eviction, explicit Clear and no disk persistence. TUI-overwrite fixtures must recover visual frames without sending input/replaying commands. Stress many sessions and verify memory returns after clear/end; no transcript index or process resurrection. |
| Inline images | After B text geometry and measured renderer baseline. Pick one documented protocol and test malformed/partial streams, decoded-pixel/image/cache caps, eviction and long animation floods. Preserve text, selection and TUI input when unsupported. Record native GPU/backend results before promoting. |
| Local triggers | After alerts and bounded search primitives. Highlight/notify only initially; compiled regex and per-session input/event budgets, explicit invalid-rule errors, flood CPU/RSS tests, and silent disabled behavior. No automatic command execution. |
| tmux integration | Separate remote-session specification/prototype. Define remote pane/window IDs, disconnect/reconnect/control ownership and the distinction between tmux detach and local Opterm view detach. Test a real local tmux server first. Do not fold remote control into ordinary view movement without an accepted contract. |
| Broadcast input | After pane transfers/control correctness. Explicit temporary recipient set, always-visible status and exit, keyboard escape, per-dispatch live ownership checks and bounded queues. Test detach/end/control loss during input and show exactly which sessions received bytes. Default ordinary input remains single-pane; never auto-restore broadcasting after restart. |

Keep SQLite, durable direct-command bookkeeping, public API/CLI/MCP execution integration, indexed history/export, and persisted clipboard history deferred. No embedded browser, AI/provider runtime integration, toolbelt, or permanent IDE panels. PowerShell shell integration, per-profile fonts, ligatures, modern keyboard reporting and unsupported native hotkey paths must be tracked honestly as separate gated work when their prerequisites are missing.

## Verification and delivery

For every Rust feature and again after each integrated wave:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

When renderer/input changes, build the native binary and run the applicable focused Linux smoke route with prerequisites available:

```sh
cargo build --locked
python3 scripts/smoke_gui.py --everyday-only
python3 scripts/smoke_gui.py --priorities-only
```

Extend focused routes for the new feature rather than relying solely on existing coverage. Use [native platform checks](platform-native-checks.md) for Wayland, macOS Apple Silicon/Intel and Windows x64. Record skipped checks with their reason. A native CI compile/package is separate from GUI interaction, screen-reader usability and GPU measurements.

Each handoff/PR reports base SHA, owned files, public APIs, limits/defaults/migration, meaningful check results, screenshots where relevant, and unresolved native evidence. Preserve Cargo.lock and detailed commits. Update implementation status only after the corresponding code and checks exist. Compare performance using the current P1 reports and matching release workloads; do not claim benchmark superiority from a private software display.

## Copy-paste cloud kickoff prompt

Replace `P1_BASE_SHA` with the final locally delivered P1 commit before sending this:

```text
Continue Opterm from P1_BASE_SHA in this repository. Verify that commit is available
and that the checkout contains all eight completed P1 development slices; report
the actual base SHA. Read AGENTS.md, docs/ui-settings-spec.md,
docs/prototype-plan.md, docs/prototype-status.md, docs/iterm2-roadmap.md, and
docs/cloud-nice-to-haves-handoff.md. The design interview is closed.

Implement the remaining P2 roadmap items 9-16 using the handoff's A/B/C waves.
Use subagents in parallel where available, with exclusive file ownership and
isolated branches/worktrees. Keep one coordinator responsible for shared UI,
settings, terminal/service protocol, Cargo/lock and integration. Workers share
APIs before wiring; integrate sequentially and make one reviewable feature PR
per complete item. Do not report helper-only branches as finished product work.

Start with key capture/maps, tagged/searchable profiles, and advanced paste.
Then implement Unicode/font fidelity, arrangements/pane movement, and opt-in
activity/completion alerts. Finish optional tab appearance and a dedicated hotkey
window with native adapters. Preserve the compact terminal-first UI, normal Tab
and Ctrl+C, exact ordinary paste without extra Enter, global fonts, live service
session identity/control, and the distinction between Close view and End session.
Keep all new buffers, regex work, event queues and caches bounded and tested.

Follow the document's concrete acceptance checks and dependency gates. Run cargo
fmt --check, strict all-target Clippy and all-target tests, plus focused native
GUI checks for renderer/input changes. Track Cargo.lock. Produce screenshots and
honest platform evidence; cloud Linux checks do not prove macOS/Windows GUI,
Wayland compositor hotkeys, audible accessibility or hardware-GPU performance.
Fix regressions before advancing to the next wave. Follow current AGENTS.md for
detailed commits and coordinator-owned final delivery; do not publish a release.

After P2 compatibility/native validation, prepare bounded P3 evaluation PRs for
instant replay, inline images, local highlight/notification triggers, separate
tmux integration, and explicit temporary broadcast input. Keep these prototypes
separate from default runtime until their budgets, behavior and promotion
decision are reviewed. Leave SQLite, durable command ledgers, public API/CLI/MCP
execution, indexed history/export, persisted clipboard history and permanent
IDE/browser/provider panels deferred. Do not expand scope to per-profile fonts,
unnegotiated keyboard reporting or ligatures without the documented prerequisites.

Report implemented behavior, tests, resource limits, commit/PR boundaries and
remaining native validation after each wave. Continue authorized work without
reopening settled design questions.
```
