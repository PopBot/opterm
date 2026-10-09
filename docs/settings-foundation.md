# Settings foundation

Open **… → Settings…**, or press **Ctrl+Shift+,** (Linux/Windows) / **Cmd+,** (macOS). Settings use a separate native window with the approved four sections. Appearance is editable; the other sections describe current behavior and clearly identify controls still to come.

The appearance draft supports system/dark/light application chrome, installed monospace fonts, font size, independent horizontal/vertical padding, line height, and outline/bar/underline cursor. **Save** validates, writes, and applies it to the open terminal. **Cancel** or closing the settings window discards the draft. Shells, agents, and the session service keep running. Terminal ANSI/256/RGB colors are preserved; profile color themes are the next slice.

## Configuration and recovery locations

The active file is `settings.toml` in the platform configuration directory's `settings/` subfolder. The backup is `last-valid-settings.toml` in a separate application-data `recovery/` folder, never in cache.

| OS | Active settings | Recovery backup |
| --- | --- | --- |
| Linux | `$XDG_CONFIG_HOME/opterm/settings/settings.toml` (usually `~/.config/opterm/settings/settings.toml`) | `$XDG_DATA_HOME/opterm/recovery/last-valid-settings.toml` (usually `~/.local/share/opterm/recovery/last-valid-settings.toml`) |
| macOS | `~/Library/Application Support/opterm/settings/settings.toml` | `~/Library/Application Support/opterm/recovery/last-valid-settings.toml` |
| Windows | `%APPDATA%\opterm\config\settings\settings.toml` | `%LOCALAPPDATA%\opterm\data\recovery\last-valid-settings.toml` |

The window shows the actual configuration path. **Edit configuration…** uses its registered desktop application; **Open backup folder** opens the recovery directory. Save once to create a missing settings file. `OPTERM_CONFIG_DIR` and `OPTERM_RECOVERY_DIR` override these two leaf directories for isolated development or custom placement. These do not change `OPTERM_STATE_DIR` or switch session services. The accepted folder picker, remembered storage location, migration, and cache controls remain future work.

```toml
version = 1

[appearance]
chrome = "system"
font_family = "System monospace"
font_size = 14.0
padding_x = 6.0
padding_y = 6.0
line_height = 1.2
cursor = "outline"
```

Chrome accepts `system`, `dark`, or `light`; cursor accepts `outline`, `bar`, or `underline`. Font size is 8–40 logical pixels, padding 0–32, line height 1–2. Unknown fields, unsupported versions, nonfinite/out-of-range values, and files larger than 64 KiB are rejected. An unavailable font falls back to an available system monospace face. Saved settings are global across desktop instances; each instance checks external edits about once a second. The preview illustrates spacing with the currently installed application font; a newly selected family takes effect on Save.

## Last-valid backup behavior

- Valid configuration loads and valid external edits refresh the recovery copy. Invalid edits keep the currently applied settings and leave the good backup intact; the settings window explains the error and location.
- At startup, missing or invalid configuration loads a valid backup if available. The broken/missing active file is left untouched. If neither file is valid, built-in defaults are used with an explicit Settings notice.
- **Restore last valid settings** loads the validated recovery copy into the draft. Review it, then **Save** to restore the active file; **Cancel** leaves the file alone.
- If the file changed while a draft was open, Save refuses to overwrite it. Choose **Reload file**, or explicitly **Replace file with my draft**.
- Save validates and stages both files before replacement. Each temporary file is flushed and each replacement is atomic. If preparing either destination or replacing the active file fails, the prior backup remains unchanged and the draft stays open for retry. Unix directory entries are flushed after replacement.
- The two files are not one filesystem transaction. A crash between replacements can leave newer valid settings and an older valid backup. The next successful load refreshes the backup from the valid settings file. A backup replacement/durability failure is reported explicitly; the app does not claim recovery was updated when it failed.
- A small process lock serializes Opterm writers. External editors do not honor that lock; contents are compared just before committing to reduce conflicts, without claiming filesystem compare-and-swap guarantees.

On Unix, directories must be private and user-owned (0700); atomic files use 0600. Windows directories receive an owner-only inherited DACL. Native Windows ACL/runtime validation remains pending. No database, command ledger, process backup, or secret-bearing profile environment is added.

## Verification

The Rust settings tests cover valid round trips, missing/corrupt startup recovery without overwriting the file, invalid drafts and external edits, explicit conflict resolution, staging failures, interrupted/failed atomic replacement, and Unix permissions. The isolated Linux X11 GUI check exercises real Save/Cancel, live font resize with the same shell PID, conflicting file edits, explicit replacement, corrupt-file restore, failed Save/retry, and closing the settings window. All GUI settings, recovery, and service paths are temporary; user preferences and live services are not modified.
