# Opterm UI, settings, and first-version specification

Status: Q54-Q83 accepted. The design interview is closed; no further design round is required before a prototype. Opterm is standalone: earlier Crateon reuse is superseded. Routine reversible interface details will be resolved in the prototype. See [the design interview](design.md). This is the approved design specification; [prototype status](prototype-status.md) distinguishes implemented behavior from remaining requirements.

## First version — Q54 accepted with additions

The first usable version includes these everyday terminal capabilities:

- Multiple windows open simultaneously, each supporting multiple tabs.
- Vertical and horizontal splits, including both orientations in one tab.
- Renamable tabs and keyboard shortcuts for terminal navigation and actions.
- Named session profiles for ordinary shells and existing CLI agents.
- Built-in themes and custom profile themes using supported appearance fields.
- Persistent local sessions, a live/detached session list, and reattachment.
- Copy/paste, scrollback search, essential settings, and simple local preference/layout/history files.

Target macOS, Linux, and native Windows; record native coverage separately. A smaller internal development slice may precede this usable version, but a one-window build does not satisfy the accepted scope.

Defer SQLite, durable direct-command bookkeeping, the public execution API/JSON CLI/MCP integration, indexed history/export, and elaborate agent management. Internal communication with the session service remains necessary. This changes the delivery order of earlier backend requirements; saved files do not imply the deferred command-durability guarantees. Live sessions survive loss of desktop views while the service remains alive; reboot or service failure does not trigger automatic command replay.

## Main window — Q55 starting layout accepted

The user found both earlier sets of proposals too heavy and too similar to a desktop IDE. The supplied screenshot is now the primary composition reference: a small native title bar, a compact session-tab row, and one continuous terminal surface underneath. Start with one terminal view; open vertical/horizontal splits only when requested. Ordinary shell and agent sessions use the same navigation.

Use modest neutral tabs and controls, avoiding a prominent app wordmark, colored new-tab button, permanent sidebar, extra toolbar, pane headers, badges, and application status strip. A CLI agent's text, input prompt, and terminal-drawn status are content inside the terminal; Opterm should not duplicate them as surrounding app widgets. The reference's specific Chat/Diff/Code Review labels do not define Opterm features or require an embedded agent UI.

Expose session profiles, live/detached sessions, new-window, splits, settings, and tab renaming through menus and keyboard shortcuts. Session discovery remains part of v1; use a compact picker and review its presentation in the prototype. The previous sidebar-heavy and always-three-pane arrangements are superseded, not accepted defaults. Keep all Q54 capabilities even when hidden in the initial view.

Q60 accepts a compact quick-access model: plus opens a new session with the default profile; an adjacent dropdown selects another profile. A small session picker lists existing live/detached work and reconnects to it. Selecting a profile starts new work; selecting an existing session reattaches to its current program. Routine menu labels and presentation will be resolved in the prototype; Q80 supplies the core bindings.

Closing a tab/pane detaches its view; ending work is a separate action, as already agreed. The reference-based PNG is the accepted starting layout. Apply a small reduction in terminal edge padding and row spacing, retaining readable text and the accepted chrome. Closing a window detaches its views under the accepted desktop-close contract; focus details follow native conventions and will be checked in the prototype.

Q62 accepts drag reordering, moving tabs between windows, and dragging out a tab to create a window. Preserve the tab's sessions, splits, and manual name without restarting programs. Manual names remain until explicitly changed/reset. Window closing follows the same detach contract; native focus behavior will be checked in the prototype.

Q71 accepts pointer/keyboard split resizing and pane switching, temporarily maximizing a pane, and restoring its prior split layout. Q80 supplies the core split shortcuts; remaining navigation bindings are reversible implementation details. Closing a pane detaches its view rather than ending its session.

### Interior density refinement

The user requested a slightly more compact interior, not a smaller font or another layout, and approved the resulting [compact PNG](design/mockups/reference-thin-terminal-compact.png) as the visual baseline. The edit targeted smaller edge insets and roughly 7–8% tighter line spacing; these are illustration targets, not accepted final numeric defaults. Font size, padding, and terminal line height will be adjustable in the appearance settings.

Terminal line height changes the height of each cell row. Blank rows and indentation emitted by a shell or CLI agent remain part of its output; Opterm must not strip or rewrite them to make the screen look denser. This distinction preserves the behavior of full-screen terminal programs.

### Appearance defaults — Q69 accepted

| Setting | Starting default |
| --- | --- |
| Font | Available system monospace font |
| Text size | 14 px |
| Terminal padding | 6 px on each edge |
| Line spacing | 1.2 times text size |
| Terminal themes | Independent neutral Dark and Light |
| Application chrome | Follow the system appearance |

Font, size, padding, and line spacing are adjustable global settings; profiles retain their selected terminal color theme. Font fallback and scaling details still require native validation. These values are starting defaults, not measured performance or readability optimums.

## Visual style — Q56 starting direction accepted

Use the user's screenshot as the direction rather than continuing the A/B/C selection. The accepted starting PNG shows small neutral macOS title/tab chrome above one opaque charcoal terminal displaying a fictional CLI-agent session. Preserve that layout with independently selected fonts and neutral terminal colors. The screenshot is a macOS visual reference; Windows/Linux window controls and conventions still need their platform-specific treatment.

See [the mockup files and generation prompts](design/mockups/README.md). Both earlier sets are rejected as too IDE-like. The starting character and layout are now accepted; Q69 supplies appearance defaults; focus/selection, accessibility, and platform variants require prototype and native validation. Raster concepts are not functioning application screenshots or evidence of rendering speed.

## Standalone product styling

The latest user instruction excludes anything Crateon-related from the app and supersedes the earlier reuse request. No related branding, named presets, token-package dependencies, application code, logos, reference screenshots, or integrations belong in Opterm or its distribution. Use Opterm-owned neutral styling and independently chosen fonts/colors.

The approved compact PNG remains a layout/density reference, not a palette/font dependency. Earlier source references and prompts are historical design records only and are excluded from runtime assets. The UI toolkit remains open; product independence does not select a renderer or framework.

## Settings — Q57 accepted

Offer a settings UI and a readable TOML file representing the same settings. Start with global preferences and named session profiles; preserve workspace working directories without introducing a general multi-level override hierarchy.

Q58 accepts a compact separate settings window with four sections: Appearance, Shortcuts & clipboard, Profiles, and Sessions & history. It does not permanently occupy space in the terminal.

Q79 accepts the [standalone settings PNG](design/mockups/settings-appearance.png) as the layout direction, with native controls on each platform. Preserve the accepted four sections, compact form, Save/Cancel, configuration access, and Restore last valid settings action. Other section contents follow the accepted fields; the raster preview is not implementation or native accessibility evidence.

A session profile is a reusable launch preset, not a saved running process. It may choose an ordinary shell or CLI agent and a supported profile theme. Q59 accepts global font, font size, horizontal/vertical padding, terminal line height, and application theme, with per-profile terminal color themes including custom supported colors. Terminal views use their selected profile's color theme while application chrome follows the global theme. This is limited appearance customization, not general configuration inheritance, executable theme plugins, or custom UI code. Fonts and spacing remain global initially.

| Group | Accepted field groups |
| --- | --- |
| Appearance | Global theme, font family/size, cursor, horizontal/vertical padding, terminal line height; built-in and custom profile themes |
| Keyboard and clipboard | App shortcuts, copy/paste behavior, relevant permissions |
| Session profiles | Shell/agent program, working directory, explicit environment overrides, selected theme |
| Sessions and history | Restore behavior, background-session behavior, scrollback/history limits, recording controls |

Q61 accepts editable platform-appropriate keyboard defaults and reset controls. Preserve ordinary shell keys unless explicitly rebound, flag duplicate app bindings, and require an explicit reassignment. Q80 accepts the defaults below.

### Shortcut defaults — Q80 accepted

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| New tab / new window | Cmd+T / Cmd+N | Ctrl+Shift+T / Ctrl+Shift+N |
| Close current view | Cmd+W | Ctrl+Shift+W |
| Copy / paste / search | Cmd+C / Cmd+V / Cmd+F | Ctrl+Shift+C / Ctrl+Shift+V / Ctrl+Shift+F |
| Split right | Cmd+D | Ctrl+Shift+E |
| Split below | Cmd+Shift+D | Ctrl+Shift+O |

Q81 accepts program-supplied automatic tab titles, falling back to the profile name. Manual names win until reset to automatic and survive tab movement and reattachment.

2026-10-10 accepted correction: recognize running Claude, Codex, Copilot, and Antigravity CLI sessions automatically, display their name and a compact provider logo in the tab, and restore the shell title on agent exit. Manual names still win. Include editable general launch profiles with provider identity and distinct terminal color themes; provider logos may also appear in the launch picker and settings. Fonts remain global under Appearance. This adds no provider account/API integration or surrounding agent panels.

Q82 accepts starting each new split as a separate session with the default profile; the split menu can select another profile. Starting directories follow Q75.

Q83 accepts preserving pasted text and line breaks without appending an extra Enter. The shell or running program handles embedded line breaks; protocol compatibility will be tested during implementation.

Q77 accepts explicit copy/paste, with copy-on-select optional and initially off. Ctrl+C remains terminal interrupt unless deliberately rebound. App copy/paste uses platform-appropriate bindings; terminal-triggered clipboard operations retain Q51's user-gesture/permission contract.

Q78 accepts 10,000 live scrollback lines per session initially, configurable. Determine an additional memory/byte bound during evaluation so the line count alone does not imply unlimited memory. Saved output is enabled by default with per-session recording controls. Retain the previously accepted 30-day, 5-GiB-total, 256-MiB-per-stream output limits. Saved output and live scrollback are separate; history cleanup does not end work. Simple first-version log files do not introduce the deferred command ledger or indexed history/export.

Q63 accepts applying saved appearance/shortcut changes immediately. Program, starting-directory, and environment changes in launch profiles affect newly started sessions; current programs keep running. Theme edits update views using that theme without restarting work.

2026-10-10 accepted correction: clicking editable terminal text moves the input caret to that position. Preserve drag selection and application mouse reporting. Each profile, including built-in Shell, has a blinking-caret toggle and a caret shape choice with Block and Line options. Saved caret changes apply immediately without restarting sessions; existing global cursor preferences remain the fallback for profiles without a shape override.

### Global-settings recovery — Q64 accepted with backup requirement

Keep a validated last-good global-settings snapshot in the current user's app-data/state area, separate from the actively edited TOML file, with access limited to that user. The exact OS paths will be selected during implementation. Validate before updating the snapshot and use a safe temporary-file/replacement procedure that retains the prior good copy on a failed or interrupted write. Invalid edits must never replace the good backup. Report backup-write failures rather than implying recovery is available.

While running, invalid file edits retain the last valid settings and show a useful error with the file location. On startup with missing/corrupt configuration, load the valid snapshot when available and explain the recovery. Offer Restore last valid settings and access to its location in the settings window; do not silently overwrite the user's broken file. If neither configuration nor backup is valid, use built-in defaults with an explicit notice. The recovery snapshot preserves the existing exclusions for marked secret environment values.

This is a small settings file and recovery copy, not a database or session-process backup. Q69 supplies appearance defaults; Q67-Q68 settle theme fields and the editing model. Resolve routine labels and error presentation in the prototype, retaining the accepted recovery requirements.

### Startup — Q65 accepted

Restore the last window/tab/split layout by default, reconnecting surviving sessions. Show ended/unknown work without rerunning its commands. On a fresh setup, open one default-shell session. Restoration can be disabled.

### Profile management — Q66 accepted

Provide create, duplicate, rename, edit, delete, and choose-default controls. Profiles contain a name, executable/arguments, starting directory, explicit environment overrides, and selected terminal theme. Deleting a profile does not end sessions already launched from it. Q72 accepts reverting to a built-in default-shell profile if the default preset is deleted and a built-in neutral theme if a referenced custom theme is deleted; explain the changed selection without interrupting running programs. Q75 accepts profile directory → workspace default → home when neither is configured, with an actionable error for an unavailable configured directory. Existing sessions retain their captured launch configuration; remaining presentation details will be reviewed in the prototype.

### Themes — Q67 accepted with standalone qualifier

Include neutral Dark and Light presets and custom foreground/background, cursor, selection, and the 16 ANSI colors. Provide a color editor and TOML theme import/export. No other-product named presets or token/code dependencies. Fonts, spacing, and application chrome remain global. Theme files contain supported color data rather than executable plugins.

### Settings editing — Q68 accepted

Use an editable draft with Save/Cancel. A valid save applies changes according to Q63 and maintains the recovery snapshot. Validate external TOML updates before applying them. If the file changes while the GUI has an open draft, offer reload or an explicit replacement decision rather than silently overwriting edits. Cancel discards the GUI draft. File-write failure must not be presented as a successful save; keep the draft and provide an actionable error so the user can correct the issue and retry.

### Storage locations — Q70 requirement accepted

Support a user-selected directory for Opterm's settings and cache. These application storage locations are separate from a shell/profile starting directory. Settings include the persisted global configuration and profile/theme definitions; cache contains recreatable data. Saved history, workspace layouts, the recovery snapshot, and live-session state are not disposable cache.

Q73 accepts one chosen Opterm folder with separate settings/ and cache/ subfolders, a folder picker, and remembered startup discovery. Keep the last-valid recovery copy separately in the normal app-data/state location. The exact locator format remains an implementation detail. The backup must not become a cache file or be removed by maintenance.

Q76 accepts copying validated settings to a new selected folder, preserving original files, and activating the location on the next desktop launch. Recreate cache as needed at that location; keep the recovery copy separate and leave live sessions running. Do not restart the session service. An unavailable selected location is reported with recovery/choose-location actions rather than a silent location change.

Q74 accepts Clear cache, limited to recreatable data. It preserves settings, profile/theme definitions, history, workspace layout, backups, and running sessions. Saved history and durable state remain distinct from cache; their exact storage layout is still to be specified.

## Next step

The interview is closed with Q54-Q83 accepted. No more product decision rounds are required before the terminal-first prototype in [the prototype plan](prototype-plan.md). Resolve routine reversible UI details there and bring back only material scope changes or evidence-based technical tradeoffs. The first working slice uses a provisional egui/eframe renderer. See [prototype status](prototype-status.md); final toolkit selection and remaining native coverage still require evidence.
