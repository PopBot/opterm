# Launch profiles and terminal themes

Open **… → Settings… → Profiles**. The compact section has two views: **Launch profiles** and **Terminal themes**. Both edit the same Save/Cancel draft as appearance settings.

## Profiles

Add, duplicate, rename, edit, or delete a preset. Use **Use as default** to choose the preset for new tabs, splits, and windows. The built-in **Shell** is a protected fallback; duplicate it to customize it.

Each profile has a name, executable, argument list, starting folder, environment overrides, and terminal theme. A blank executable uses the default system shell. Each argument row is one argument, preserving spaces; arguments are passed directly to the executable and are not parsed as a shell command. Use an explicit shell executable and `-c`/equivalent if shell interpretation is desired.

Starting folders use **profile folder → workspace folder → home**. Paths must be absolute or begin with `~`. An unavailable configured folder or executable reports a launch error; it does not silently switch directories or create a replacement process. Environment overrides apply after the usual interactive terminal environment, so an explicit profile override can deliberately change TERM, COLORTERM, or NO_COLOR.

Ordinary **+**, New tab, split shortcuts, and New window use the default preset. Right-click **+** to choose a profile, or use **… → New tab with profile / Split right with profile / Split below with profile**.

A session captures its profile name, launch configuration, and chosen theme ID. Editing a profile affects future launches. Existing shells keep their executable, arguments, folder, environment, and selected theme. Program-supplied automatic titles take precedence; the captured profile name is the fallback, and manual tab names still win. Deleting a profile does not stop its processes. Deleting the default preset reverts to built-in Shell and explains the change.

## Themes

Neutral **Dark** and **Light** are built in. Duplicate either, or add a custom theme, then edit foreground, background, cursor, selection, and the 16 ANSI colors. Choose the theme in a profile. Fonts, spacing, cursor shape, and app chrome remain global.

Saving a theme edit updates sessions using that theme, including service-owned detached sessions, without restarting work. Ordinary named colors and ANSI palette entries use the profile theme. Explicit program RGB colors, indexed colors 16–255, and program OSC color overrides retain their meaning. Default background/cursor and color-query replies respect program overrides. A deleted theme falls back to Dark for profiles and live sessions; running programs continue.

**Import / export TOML theme** accepts a full file path. Import validates a data-only theme, assigns a fresh ID, and adds it to the draft; Save applies it. Export writes the selected theme to a new file and refuses to overwrite existing files. All 16 ANSI colors and valid `#RRGGBB` values are required. Theme files do not run code.

## Settings and recovery

Profiles and custom themes live in the existing global `settings.toml` and use the same validated last-valid recovery backup, atomic replacement, Save/Cancel, external-edit conflict handling, and Restore action. See [locations and recovery guarantees](settings-foundation.md).

The format is now version 2. Version-1 appearance-only settings and backups still load; the active file stays untouched until Save, which writes version 2. For example:

```toml
version = 2
default_profile = "coding"
workspace_directory = ""

[[profiles]]
id = "coding"
name = "Coding agent"
executable = "codex"
arguments = []
directory = "~/Dev"
theme = "dark"
environment = []
```

The UI generates stable random IDs; renaming does not change references. Built-in IDs `shell`, `dark`, and `light` are reserved. Profile/theme names must be unique, and IDs may not repeat. Configuration remains bounded to 64 KiB, with up to 64 custom profiles/themes, arguments per profile, and environment overrides per profile. Invalid edits never replace a good backup.

Mark an environment value **Secret** to hide its editor text and omit its value from the recovery copy. The active private TOML still contains the value in plaintext. Recovery preserves the variable name and secret marker but clears the value; reenter it before launching that profile. Ordinary environment values, profiles, and themes recover normally. This is not a credential vault, and values entered in argument rows do not have secret markers.

## Existing service versions

The desktop checks a capability flag on the existing protocol handshake. A service started by a pre-profile binary keeps its live work usable. Unsupported profile/theme requests are rejected locally, without sending an unknown request, disconnecting, restarting, or replaying work. The desktop explains that the service is older.

Once its work is finished, deliberately stop the old service and reopen Opterm to start the rebuilt one. The command ends **all** sessions in that service; do not use it to upgrade while work should keep running:

```sh
./target/release/opterm --stop-service
./target/release/opterm
```

An isolated `OPTERM_STATE_DIR` is another way to try the new service without touching old work. Keep each service's desktop instances on the same settings/recovery locations for consistent global theme updates.

## Validation and remaining coverage

Real PTY/service checks cover profile arguments/environment/folder, launch errors, retained profile identity on reattachment, live theme changes and fallback with the same shell PID, explicit RGB/program palette overrides, and color-query replies. Settings checks cover version-1 migration, profile/theme recovery, marked-secret exclusion, invalid values/size, and theme import/export without overwriting files. A preserved prior release was also tested: existing work remains usable after a locally rejected profile request.

The Linux X11 GUI check creates and saves a default profile through the native controls, launches it in the selected folder, verifies Light/custom background pixels, edits a live theme while checking the same PID, launches a default-profile split, and deletes the theme/default profile without stopping those shells. It also reruns the existing settings, clipboard, layout, and session-continuity checks using isolated service/preferences/recovery directories. It requires Python 3.11+ for the standard-library TOML reader.

macOS, Windows, Wayland, authenticated interactive agent behavior, IME/accessibility, and comparative performance remain unverified. This slice adds no database, saved transcript index, or process replay.
