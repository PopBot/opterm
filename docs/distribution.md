# Unsigned prototype packages

Opterm packages are native builds, not installers. Download the archive and checksum from the successful **Native checks** workflow's `opterm-<platform>-<architecture>` artifact at https://github.com/PopBot/opterm/actions/workflows/native-checks.yml. GitHub may wrap the files in an artifact ZIP; extract that first. Artifact downloads require GitHub access and expire after 14 days. No GitHub release is published.

| Platform | Package | Run after extraction |
|---|---|---|
| Linux x86_64 | `opterm-0.1.0-linux-x86_64.tar.gz` | `./opterm` inside the extracted folder |
| macOS Apple Silicon | `opterm-0.1.0-macos-aarch64.tar.gz` | Open `Opterm.app`, or run `Opterm.app/Contents/MacOS/opterm` |
| macOS Intel | `opterm-0.1.0-macos-x86_64.tar.gz` | Open `Opterm.app`, or run its executable |
| Windows x86_64 | `opterm-0.1.0-windows-x86_64.zip` | Run `opterm.exe` inside the extracted folder |

## Verified native archives

[Run 38031780232](https://github.com/PopBot/opterm/actions/runs/38031780232) passed all four native check jobs and all four release packaging jobs for `1db391c`. The Linux release archive additionally passed an extracted native controls/session-preservation check. Download the corresponding Actions artifact:

- [Linux x86_64](https://github.com/PopBot/opterm/actions/runs/38031780232/artifacts/11663140371)
- [macOS Apple Silicon](https://github.com/PopBot/opterm/actions/runs/38031780232/artifacts/11662181636)
- [macOS Intel](https://github.com/PopBot/opterm/actions/runs/38031780232/artifacts/11662636090)
- [Windows x86_64](https://github.com/PopBot/opterm/actions/runs/38031780232/artifacts/11663145439)

These artifacts identify their source revision and binary digest in `BUILD.json`. This validates native construction and exercised test paths; Mac/Windows physical desktop controls remain outside this run.

The macOS app is unsigned and not notarized. Gatekeeper may require approval through **System Settings → Privacy & Security** after the first attempted launch. Windows may show a reputation warning for the unsigned executable. Use packages only from the repository's verified workflow run; signing and automatic updates are deferred. Hosted native build/tests do not establish full macOS/Windows desktop acceptance.

Linux packages are built on Ubuntu 24.04 and require its compatible C runtime and desktop libraries, including X11/Wayland, OpenGL, xkbcommon and xkbcommon-x11. They are not static builds for every Linux distribution. Install fonts for CJK coverage where needed; bundled emoji are monochrome. Windows requires Windows 10 version 1809 or later for ConPTY. See `USER-GUIDE.md` for settings, profiles, session persistence and limitations.

Settings, backups and sessions stay in normal user directories or your selected settings root; extracting a package does not migrate them. Keep the executable in a stable location while its session service runs. Close windows to detach, then reopen the same executable to reconnect. A reboot or service failure can end the processes. Replacing/moving the executable while sessions run is not an upgrade mechanism.

To build/package locally on the destination platform:

```sh
cargo build --release --locked
python3 scripts/package.py
```

On Windows, `python scripts/package.py` is equivalent. Output appears under `target/dist/`, with a SHA-256 sidecar. The script packages an existing binary and never builds, installs, signs, or publishes it. Archive entries have normalized timestamps/permissions and fixed ordering for repeatable packaging of the same binary, source commit and resolved dependencies. Native compiler output is not claimed to be reproducible across machines.

Each package includes Opterm's MIT/Apache-2.0 texts, runtime documentation, font licenses, build metadata and a host-filtered dependency inventory. Original license files from cached crates and pinned supplemental upstream texts are copied unchanged. Dependencies retain their own license expressions and sources; the inventory conservatively includes resolved build/optional dependencies rather than asserting every crate is linked. Historical design assets are excluded.

Packaging uses [GitHub's official artifact action](https://github.com/actions/upload-artifact). Gatekeeper approval guidance comes from [Apple's security documentation](https://support.apple.com/en-us/102445); ConPTY availability is documented by [Microsoft](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole).
