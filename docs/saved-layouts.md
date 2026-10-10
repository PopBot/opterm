# Saved windows, tabs, and splits

Ordinary startup restores saved windows by default. Each window restores its tab order and manual names, split orientations and ratios, active tab, focused pane, temporary pane maximization, window size/position, and native maximized state. Existing open windows keep their own views; their saved files are locked so another desktop cannot overwrite or restore the same window.

Restoration only attaches references to existing service-owned sessions. A per-service random identity prevents a saved session number from attaching to different work after the service restarts. Exited sessions retain their available terminal screen. Missing sessions appear as unavailable panes in the saved arrangement; their commands are never replayed and no replacement shell is created. Close an unavailable pane or use **+** to start new work explicitly.

On first use without saved windows, Opterm attaches available detached work, or creates one default-profile session if there is none. **New window**, `--new-window`, and an explicit executable launch start a fresh window. Detached sessions remain available through **Sessions**; reopening a saved window attaches only its own sessions.

Turn off **Settings → Sessions & history → Restore saved windows, tabs and splits on startup** to start fresh on ordinary launches. Save/Cancel and the existing last-valid-settings backup apply to this setting. The one-launch `--no-restore` flag also starts fresh without changing preferences. Existing saved windows remain on disk when restoration is disabled.

## Files and failure behavior

Saved layouts use private, validated JSON files under `<OPTERM_STATE_DIR>/layouts/`, or the normal runtime directory's `layouts/` folder. On Linux the normal location is `~/.local/share/opterm/runtime/layouts/` (subject to XDG configuration). Layouts are durable state, separate from recreatable cache and from the settings recovery backup.

Each `window-<random-id>.json` contains layout geometry, names, and service/session references. It contains no executable, arguments, environment, command history, or terminal transcript. Closed windows remain saved for the next launch; unused saved windows can be removed by deleting their JSON files while those windows are closed. There is no database or shared transcript index.

A desktop keeps its own window lock while open, writes changed layouts atomically about once a second, and saves the current arrangement on normal close. A crash can lose the most recent second of layout changes; service-owned processes continue normally. Other windows write separate files. Malformed or unsupported saved files are left untouched and reported. Write failures keep the previous valid file and show an error. Prototype bounds are 64 saved windows, 128 panes/tabs per window, 32 split levels, and 256 KiB per file.

Window positioning follows native toolkit support; Wayland does not support restoring absolute positions. Off-screen placement after monitor changes, native macOS/Windows behavior, and Wayland runtime coverage remain unverified.

## Updating an existing service

A service started before this slice has no persistent service identity. Existing work remains usable, with a notice; the app cannot safely restore its saved references automatically. Reattach that work through **Sessions**. After its work is finished, deliberately stop the old service and reopen to use the updated service. `--stop-service` ends all of its sessions. Opterm never stops or upgrades a live service automatically.

## Verification

Rust checks cover layout round trips, nested geometry/focus, independent window ownership, invalid/corrupt files, bounds, private permissions, failed writes, selected-session reattachment, and rejection of reused session numbers after service restart. The Linux X11 GUI check covers normal close and desktop-crash continuity, two-window restoration with retained shell PIDs/environment and split focus, manual-name restoration, and unavailable panes without replacement sessions after service restart.

The GitHub remote is `git@github.com:PopBot/opterm.git`; local `main` tracks `origin/main` and GitHub's default branch is `main`.
