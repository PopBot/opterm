# App icon and desktop installation

Opterm now carries its own application icon. One square PNG generates the native formats for all three platforms. The generated assets are tracked, so ordinary builds and packaging do not require Python image libraries.

## Replace the icon

1. Create a square PNG at least 1024 × 1024 pixels. Transparent corners work well.
2. Keep your original artwork, then run:

   ```sh
   python3 -m pip install --user Pillow
   python3 scripts/generate_icons.py --source /absolute/path/to/my-icon.png
   ```

   If your Python installation requires a virtual environment, install Pillow there and use that environment's Python for the generator.
3. Copy your source PNG to `assets/icons/opterm.png` to make it the default regeneration source. Commit that source together with `window-icon.png`, `opterm.ico` and `opterm.icns`.
4. Rebuild and repackage **on each native platform**:

   ```sh
   cargo build --release --locked
   python3 scripts/package.py
   ```

The embedded 256-pixel PNG supplies the desktop and settings window icons. Linux packages use it with the desktop launcher and a stable Wayland application ID. Windows native builds embed the multi-resolution `.ico` into `opterm.exe` through `build.rs`. macOS packages place the `.icns` in `Opterm.app/Contents/Resources` and declare it in `Info.plist` for Finder and the Dock. Icons are build assets, so changing them requires a rebuild; there is no runtime icon setting.

## Linux application-menu installation

From the repository:

```sh
cargo build --release --locked
python3 scripts/install_linux.py
```

From an extracted Linux package:

```sh
python3 install.py
```

This installs the binary in `~/.local/share/opterm`, the icon under the user's hicolor icon theme, and an **Opterm** launcher in `~/.local/share/applications`. Launch it from the desktop application menu and pin the running application if desired. No administrator access is needed. Re-run the installer after rebuilding/updating. An already running service keeps its existing executable until deliberately stopped after its work finishes.

## macOS application installation

Extract the native package and drag **Opterm.app** into **Applications**, or `~/Applications` for a per-user install. Open it from Finder or Spotlight and drag it into the Dock if desired. The application remains unsigned; signing/notarization are separate release work.

## Windows application installation

From the extracted Windows package, run in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

The package's script copies Opterm into `%LOCALAPPDATA%\Opterm` and creates a Start menu shortcut using the executable's embedded icon. Add `-DesktopShortcut` for a desktop shortcut. Close the desktop and finish/stop the background service before replacing an executable that Windows reports as in use. Signing and installer/update infrastructure remain deferred.

## Validation boundary

Linux icon, launcher registration, terminal controls and packaging can be verified locally. macOS/Windows native icon, Dock/taskbar, and installer-script behavior for this change require their native builds and desktop checks. Existing older native-package evidence does not establish acceptance of these new assets or scripts.
