# Opt-in shell integration

Opterm recognizes explicit OSC 133 prompt, input, command-output, and completion markers. OSC 7 supplies a local working directory for relative file links. These annotations exist only in bounded live terminal scrollback; Opterm does not record a command ledger or rerun commands.

In an interactive Bash terminal, enable the bundled hooks for that shell:

```bash
source /absolute/path/to/opterm/scripts/shell-integration/opterm.bash
```

For interactive zsh:

```zsh
source /absolute/path/to/opterm/scripts/shell-integration/opterm.zsh
```

Replace the path with your checkout or installed script location. Sourcing applies to the current shell and is safe to repeat. Opterm does not edit startup files automatically. If you want the hooks in every terminal, you can add the appropriate source line to your own shell configuration after trying them.

The Bash script preserves existing `PROMPT_COMMAND` entries and refuses to replace an existing `DEBUG` trap or enable hooks while `functrace` is active. The zsh script adds `precmd` and `preexec` hooks. Prompt themes that completely replace these hooks or emit their own OSC 133 markers may need separate integration. Bash and zsh hooks have been exercised through real PTYs on Linux; PowerShell and other shells do not have bundled hooks in this development slice.

With integration active, use the terminal menu or editable shortcuts:

| Action | Linux / Windows default | macOS default |
|---|---|---|
| Previous prompt | Ctrl+Alt+PageUp | Cmd+Alt+PageUp |
| Next prompt | Ctrl+Alt+PageDown | Cmd+Alt+PageDown |
| Copy last command output | Ctrl+Shift+Alt+C | Cmd+Shift+Alt+C |

Prompt navigation scrolls to and marks a retained prompt. Manual scrolling clears that navigation marker. Copy last command output copies rendered text between the most recent completed command's markers, excluding the prompt and echoed input. It preserves hard line breaks and joins soft wraps. An attached observer can copy output; changing the shared viewport requires control. Commands without output produce an empty result. These default bindings are implemented across platforms; native macOS and Windows runtime coverage remains unverified.

Markers are capped at 128 retained prompts. Output copies are limited to 262,144 cells and 1 MiB. Trimmed output, an unfinished command, missing markers, and alternate-screen programs produce an explicit unavailable message. Resize and destructive terminal edits invalidate old boundaries; a fresh prompt establishes new ones. The last accepted working directory survives an ordinary resize. If you resize at an idle prompt, press Enter once to establish a fresh prompt before running a command whose output you want to copy.

OSC 7 directories must be existing absolute paths on the local host, with valid percent encoding and no control characters. Empty hosts, `localhost`, and the machine's hostname are accepted; remote hosts and malformed metadata clear an older directory value. This validates shell-emitted metadata rather than proving a process's directory. No relative path is resolved from guessed profile settings. Unknown shells can emit OSC 133 A/B/C/D and OSC 7 themselves.

An older running session service reports no shell-integration capability. Opterm leaves its sessions running and reports that limitation. Finish those sessions before deliberately restarting the service to adopt a newer binary, or try the new build with an isolated `OPTERM_STATE_DIR`.
