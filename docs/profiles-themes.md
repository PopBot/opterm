# Launch profiles and terminal themes

Open **… → Settings… → Profiles**. The compact section has two views: **Launch profiles** and **Terminal themes**. Both edit the same Save/Cancel draft as appearance settings.

## Profiles

Add, duplicate, rename, edit, or delete a preset. Use **Use as default** to choose the preset for new tabs, splits, and windows. The built-in **Shell** is a protected fallback; duplicate it to customize its launch. Its caret settings can be edited directly.

Each profile has a name, executable, argument list, starting folder, environment overrides, and terminal theme. A blank executable uses the default system shell. Each argument row is one argument, preserving spaces; arguments are passed directly to the executable and are not parsed as a shell command. Use an explicit shell executable and `-c`/equivalent if shell interpretation is desired.

Starting folders use **profile folder → workspace folder → home**. Paths must be absolute or begin with `~`. An unavailable configured folder or executable reports a launch error; it does not silently switch directories or create a replacement process. Environment overrides apply after the usual interactive terminal environment, so an explicit profile override can deliberately change TERM, COLORTERM, or NO_COLOR.

Ordinary **+**, New tab, split shortcuts, and New window use the default preset. Right-click **+** to choose a profile, or use **… → New tab with profile / Split right with profile / Split below with profile**.

A session captures its profile name, launch configuration, and chosen theme ID. Editing a profile affects future launches. Existing shells keep their executable, arguments, folder, environment, and selected theme. A recognized running CLI supplies the automatic provider label; otherwise program-supplied titles take precedence over the captured profile name. Manual tab names still win. Deleting a profile does not stop its processes. Deleting the default preset reverts to built-in Shell and explains the change.

**Caret shape** offers Block, Line, Outline, Underline, or Appearance default. **Blinking caret** toggles a half-second visible/hidden cycle. Save applies these choices to existing views of that profile without restarting their programs. Typing or moving the cursor makes it visible again. Blinking defaults off, and profiles without a shape override retain the global Appearance choice. Running sessions whose profile is deleted retain their captured caret preferences.

On Unix, a plain click in tracked Bash, Zsh, or Fish input moves the caret using that shell's normal arrow-key protocol, including soft-wrapped rows and wide characters. Dragging still selects text; Shift-click keeps selection behavior. Prompt text, other output, scrollback, canonical/password reads, and read-only views do not send movement keys. Applications that enable terminal mouse reporting continue to receive their own clicks. Click editing requires the current service; an older service retains selection. Explicit multiline input, tabs, unusual line-editor keymaps, and non-reporting application editors are outside this development slice. Windows shell click editing is not implemented; native macOS behavior remains unverified.

## AI profiles and automatic recognition

Claude (`claude`), Codex (`codex`), GitHub Copilot (`copilot`), and Antigravity CLI (`agy`, also recognized as `antigravity`) have editable general profiles. Each starts in the workspace folder or home, with no additional arguments or environment overrides. Installing/authenticating the CLI remains the user's responsibility; an unavailable executable reports the ordinary launch error. The profile's **AI provider** field selects its logo independently of its editable name; **Automatic** infers identity from a recognized executable.

The presets use neutral Dark backgrounds with terracotta Claude, mint Codex, violet Copilot, and blue Antigravity accents for cursor, selection, and ANSI cyan. These are Opterm theme choices, not official provider font/palette specifications. Logos are embedded locally with [asset attribution](../assets/providers/README.md). Selecting a profile in settings also selects its theme for the Terminal themes editor. Fonts remain global under Appearance; recognizing an agent does not change the shell's selected font or theme.

The service inspects session-related process names and launcher script operands every 500 ms, including detached work. It recognizes native executables and common Node/shell launchers without interpreting typed commands, prompts, or output text. On Unix it follows the PTY foreground process group and ancestry, retaining an agent label while that agent runs a tool and excluding background agents. On Windows it uses session process ancestry; ambiguous different providers yield no inferred identity, and distinguishing background work still needs native validation. Unknown launchers retain normal program titles/profile fallback.

While recognized, the compact tab shows the provider logo and automatic name; manual text remains authoritative. On exit the automatic title returns to the last shell title, or the profile fallback if the shell has supplied none. Shell prompt title updates continue normally. Logos also appear in profile choices, settings, and the Sessions picker. Provider identity refers to the running CLI, not its selected underlying model.

Version-1/2 settings gain the presets once when migrated in memory. Save persists version 3, without replacing existing profiles, themes, or the default shell selection. Version-3 deletions stay deleted; **Add missing AI profiles** restores missing presets. Migration respects the existing 64-profile/theme limits. No live process is restarted.

## Themes

Neutral **Dark** and **Light** are built in. Duplicate either, or add a custom theme, then edit foreground, background, cursor, selection, and the 16 ANSI colors. Choose the theme in a profile. Fonts, spacing, and app chrome remain global; profiles may override the global cursor shape.

Saving a theme edit updates sessions using that theme, including service-owned detached sessions, without restarting work. Ordinary named colors and ANSI palette entries use the profile theme. Explicit program RGB colors, indexed colors 16–255, and program OSC color overrides retain their meaning. Default background/cursor and color-query replies respect program overrides. A deleted theme falls back to Dark for profiles and live sessions; running programs continue.

**Import / export TOML theme** accepts a full file path. Import validates a data-only theme, assigns a fresh ID, and adds it to the draft; Save applies it. Export writes the selected theme to a new file and refuses to overwrite existing files. All 16 ANSI colors and valid `#RRGGBB` values are required. Theme files do not run code.

## Settings and recovery

Profiles and custom themes live in the existing global `settings.toml` and use the same validated last-valid recovery backup, atomic replacement, Save/Cancel, external-edit conflict handling, and Restore action. See [locations and recovery guarantees](settings-foundation.md).

The format is now version 3. Version-1/2 settings and backups still load; the active file stays untouched until Save, which writes version 3. For example:

```toml
version = 3
default_profile = "coding"
workspace_directory = ""

[[profiles]]
id = "coding"
name = "Coding agent"
executable = "codex"
provider = "codex"
arguments = []
directory = "~/Dev"
theme = "dark"
environment = []

[profiles.caret]
shape = "block" # "bar" is the Line option in the UI
blink = true
```

Built-in Shell uses a separate `[shell_caret]` table with the same fields. These optional fields load with existing version-3 files and participate in the same recovery backup.

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

Provider recognition is covered by isolated Linux PTY/service fixtures for all four CLIs: immediate title emission, foreground launch/exit, background exclusion, detach/reattach, and manual naming. The focused `python3 scripts/smoke_gui.py --providers-only` check covers native labels/logos, shell-title restoration, restored manual naming, settings presets, and saved theme identity. These fixtures do not prove authenticated behavior of each real provider or native macOS/Windows recognition.

The focused `python3 scripts/smoke_gui.py --caret-only` Linux X11 check verifies real Bash click insertion, blinking block frames, and saving a steady line caret through Profiles without replacing the shell PID. Real PTY tests cover movement in both directions, prompt/output and stale-view rejection, soft wrapping with scrolling, wide and combining characters; service checks reject observers and stale controllers. Caret settings round-trip through TOML and recovery. Other shells and native macOS/Windows caret rendering remain unverified.
