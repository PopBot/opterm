# iTerm2-inspired improvements for Opterm

Research date: 2026-10-10. These are recommendations, not implemented features or an accepted parity target. The current repair work comes first. References use current official iTerm2 documentation; the priorities and proposed Opterm behavior are our own assessment.

Keep the accepted terminal-first window, separate compact settings, platform-appropriate shortcuts, and standalone branding. Opterm closes **views** while its independent service keeps work running; **End session** remains separate. iTerm2's ordinary Close terminates a session, so matching that behavior would violate Opterm's accepted contract. [iTerm2 menu documentation](https://iterm2.com/documentation-menu-items.html)

## What Opterm already has

The current source and [user guide](user-guide.md) provide multiple windows/tabs, nested horizontal/vertical splits, divider resizing, pane focus/maximization, automatic/manual titles, tab movement, profile/theme editing, editable app shortcuts, selection/copy/paste, mouse reporting, bracketed paste, bounded live scrollback, literal per-session search, saved layouts, and service-owned detach/reconnect. Provider recognition, per-profile caret controls, and IME handling have recent implementation slices; native/platform acceptance remains separate.

The renderer already uses OpenGL. Background IPC, cached snapshots, unchanged-screen suppression, and hidden-session drawing avoidance exist. Adding “GPU acceleration” alone is not a useful next task. The [desktop performance report](performance-desktop.md) identifies measurements still missing; its private-display startup results are not hardware-GPU or keystroke-latency results. Some older status prose lags current code, so these recommendations use `src/main.rs`, `terminal.rs`, `input.rs`, `shortcuts.rs`, `settings.rs`, `profiles.rs`, and `service/client.rs` alongside the docs.

## P0 — complete the requested everyday repairs

- Visible close buttons on every tab, useful tab context menus, reliable terminal Tab/Shift+Tab delivery, reorder/tear-out/move controls, pane-edge snap previews, and a readable keyboard-shortcut editor.
- Application identity, custom icon assets, and a normal launcher/bundle on Linux, macOS, and Windows; validate each on its native OS.
- Preserve session IDs, split structure, focus, manual names, and processes during movement. Verify cancellation and failed transfers as carefully as successful drops.

This is the active implementation work, not an additional research batch.

## P1 — the next improvements with the most everyday value

| Improvement | Opterm change and user benefit | iTerm2 source |
| --- | --- | --- |
| **1. Better Find** | Add case sensitivity/smart-case, an optional regex mode, match count, and visible match highlights. Then search across open sessions with a compact results picker. Keep searches limited to bounded live scrollback; no saved-output index. Today Find is literal, case-sensitive, and limited to the focused session. | [Find controls](https://iterm2.com/documentation-menu-items.html), [Search highlights](https://iterm2.com/features.html) |
| **2. Smart selection and links** | Double-click words, triple-click lines, select rectangular regions, and recognize wrapped URLs/file paths. Add Copy link/Open link to the terminal context menu and expose OSC 8 link metadata to the renderer. Preserve Shift's local-selection override when a TUI owns the mouse. Current selection starts as a simple character range. | [Smart Selection](https://iterm2.com/documentation-smart-selection.html), [Pointer settings](https://iterm2.com/documentation-preferences-pointer.html) |
| **3. Keyboard copy mode** | An explicit mode for moving a selection by character, word, line, and screen; copy without touching the mouse. Freeze the viewed snapshot while the service continues processing output. Escape returns to ordinary terminal input. | [Copy Mode](https://iterm2.com/documentation-copymode.html) |
| **4. Quick session switcher and Undo close** | Search tabs/windows/sessions by title and profile, with recently used ordering. Reopen the last detached tab with its split layout and surviving sessions; never rerun a command if work ended. These build on Opterm's existing Sessions picker and saved-layout references. | [Open Quickly](https://iterm2.com/documentation-highlights.html), [Undo Close](https://iterm2.com/documentation-preferences-profiles-session.html) |
| **5. Prompt-aware shell integration** | Recognize prompt/input/output boundaries and current directory using explicit shell integration. Add previous/next prompt and Copy last command output within live scrollback. Replace fragile prompt inference where practical. Start with supported POSIX shells; PowerShell needs its own implementation and native proof. Keep metadata in bounded memory; no durable command ledger, history index, or replay. | [Shell Integration](https://iterm2.com/documentation-shell-integration.html) |
| **6. Readability controls** | Optional minimum contrast, context-aware cursor color, Find cursor, cursor-line guide, and modest inactive-pane dimming. Keep them user-controlled so applications' intended colors remain available. Existing theme colors/caret shape alone do not provide these aids. | [Color settings](https://iterm2.com/documentation-preferences-profiles-colors.html), [Cursor controls](https://iterm2.com/documentation-highlights.html) |
| **7. Accessible terminal interaction** | Expose terminal text, caret, selection, and focused-session identity to native accessibility APIs. Label icon-only controls, retain stable review focus during output, and bound/coalesce output announcements. Validate with VoiceOver, NVDA, and Orca separately; visual screenshots do not prove accessibility. | [iTerm2 accessibility release notes](https://iterm2.com/downloads.html) |
| **8. Measured latency and resource work** | Establish real-GPU keystroke-to-pixel and output-throughput baselines before optimizing. Then evaluate dirty-row updates, tighter idle wakeups, and adaptive redraw under output floods. Preserve prompt/input responsiveness and bounded queues. iTerm2 documents a throughput/latency tradeoff and power-aware GPU controls; Opterm should choose behavior from measurements. | [Rendering and throughput settings](https://iterm2.com/documentation-preferences-general.html) |

Recommended order: finish P0, then Better Find → Smart selection/links → Quick switcher/Undo close → Keyboard copy mode → Shell integration. Accessibility and performance measurements should accompany these batches rather than wait for the whole list.

## P2 — valuable customization and power features

| Improvement | Proposed behavior and limits | iTerm2 source |
| --- | --- | --- |
| **9. Key capture and terminal key maps** | Capture a chord by pressing it; show conflicts before Save. Separately support per-profile terminal mappings and configurable Option/Alt behavior. Add negotiated modern keyboard reporting only with conformance tests; keep ordinary shell Tab and Ctrl+C intact. | [Profile key settings](https://iterm2.com/documentation-preferences-profiles-keys.html) |
| **10. Unicode and font fidelity** | Improve grapheme/combining-mark, emoji, wide-character, fallback-font, bold/italic, and high-DPI rendering. Evaluate optional ligatures only after cell alignment and performance are proven. Retain global fonts under current scope; per-profile font overrides would be a later scope change. | [Text settings](https://iterm2.com/documentation-preferences-profiles-text.html) |
| **11. Activity and completion alerts** | Opt-in bell, new-output, idle-to-active, and exit indicators; completion alerts when shell integration supplies an actual completion boundary. Use small tab indicators and native notifications without extra permanent chrome. Let the user choose channels and suppress noise. | [Terminal alerts](https://iterm2.com/documentation-preferences-profiles-terminal.html) |
| **12. Searchable/tagged profiles** | Filter the profile launcher and settings list, add tags/favorites, and offer New tab/split with current profile explicitly. Keep existing default-profile behavior. Host/directory-based automatic theme switching can follow shell integration without changing the running process. | [Tagged profiles](https://iterm2.com/features.html), [Automatic Profile Switching](https://iterm2.com/documentation-automatic-profile-switching.html) |
| **13. Named arrangements and pane movement** | Save named layout presets, duplicate a tab's arrangement into explicitly new sessions, and move individual panes between tabs/windows. Offer menu equivalents to drag operations. Restoring a live layout reconnects sessions; launching a preset starts only programs explicitly requested by the user. | [Arrangements and duplication](https://iterm2.com/documentation-menu-items.html), [Move pane](https://iterm2.com/documentation-preferences-pointer.html) |
| **14. Advanced paste** | Optional paste preview/edit and paced paste for fragile remote programs. Plain Paste continues preserving exact text and adds no Enter. Make transformations explicit; any multiline confirmation is an opt-in preference. Start without persisted clipboard history. | [Paste Special](https://iterm2.com/documentation-menu-items.html), [Advanced Paste](https://iterm2.com/features.html) |
| **15. Hotkey terminal window** | Show/hide a dedicated terminal using a system shortcut, optionally at a chosen screen edge. Preserve live sessions. Native registration and window placement differ across Windows/macOS/X11/Wayland; document unsupported compositor behavior and provide a normal launcher fallback. | [Hotkeys](https://iterm2.com/documentation-hotkey.html) |
| **16. Optional tab appearance** | Configurable tab width, active-tab visibility, and subtle per-profile tab color. Keep a top compact tab row as the default; optional left-side tabs must not turn into a permanent IDE sidebar. | [Appearance settings](https://iterm2.com/documentation-preferences-appearance.html), [Tab color](https://iterm2.com/documentation-preferences-profiles-colors.html) |

## P3 — evaluate after terminal compatibility and native validation

- **Bounded instant replay:** recover screen contents overwritten by a TUI using a strict per-session memory budget and explicit Clear. This is a short-lived screen snapshot buffer, not indexed transcript history or process resurrection. [Instant Replay](https://iterm2.com/documentation-highlights.html)
- **Inline images:** evaluate a documented image protocol with bounded decoded pixels, image count, and cache eviction. Prove long-running animated workloads stay bounded. [Images protocol](https://iterm2.com/documentation-images.html)
- **Local highlight/notification triggers:** regex rules that highlight output or notify first; omit automatic command execution from the initial design. Bound processing cost during output floods. [Triggers](https://iterm2.com/documentation-triggers.html)
- **tmux integration:** a separate remote-session project after the local service model is solid. Map remote panes without confusing tmux detach with Opterm view detach. [tmux Integration](https://iterm2.com/documentation-tmux-integration.html)
- **Broadcast input:** only an explicit temporary mode with a visible recipient set and a prominent exit control. Normal input continues targeting one pane. [Broadcast Input](https://iterm2.com/documentation-menu-items.html)

Keep SQLite, durable direct-command bookkeeping, public API/CLI/MCP integration, and indexed history/export deferred. iTerm2's toolbelt, embedded browser/AI chat, provider API integrations, and permanent IDE panels are not recommended for Opterm's accepted thin terminal window.

## Performance evidence to collect

Run repeatable release builds on each native platform, recording hardware, GPU/backend, display scale/refresh, fonts, shell, terminal size, and workload. Separate fresh service launch, opening an additional window, and reconnection to existing sessions.

Measure input-to-visible latency (median and tail), sustained output throughput **while typing**, resize/redraw behavior, foreground and detached-service CPU/RSS, idle wakeups, and long-running memory growth. Use ordinary shells, real editor/pager TUIs, Unicode-heavy output, several panes, and many detached sessions. Compare before/after distributions under the same conditions; add a native-terminal comparison only with matched workloads and clearly stated differences.

Evaluate scrollback compression only if measured memory savings justify CPU cost; iTerm2 itself documents that tradeoff. Adaptive frame pacing, dirty-row transport, glyph caching, and compression are candidate experiments, not promised speedups. [iTerm2 rendering/scrollback controls](https://iterm2.com/documentation-preferences-general.html)

Acceptance requires Linux X11 and Wayland checks plus native macOS/Windows coverage. Cross-platform Rust dependencies and generated app icons are not proof that OS shortcuts, drag transfers, accessibility, launcher identity, or rendering behave correctly everywhere.
