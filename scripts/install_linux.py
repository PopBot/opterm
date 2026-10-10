#!/usr/bin/env python3
"""Install Opterm's binary, icon and application-menu launcher for the current user."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def desktop_quote(value):
    return '"' + str(value).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%') + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    folder = Path(__file__).resolve().parent
    repository = folder.name == 'scripts'
    parser.add_argument('--binary', type=Path, default=folder.parent / 'target/release/opterm' if repository else folder / 'opterm')
    parser.add_argument('--prefix', type=Path, default=Path.home() / '.local')
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f'Binary missing: {binary}; build with cargo build --release --locked first')
    prefix = args.prefix.expanduser().resolve()
    app = prefix / 'share/opterm'
    app.mkdir(parents=True, exist_ok=True)
    # Replace atomically so an already running service can keep its mapped executable.
    with tempfile.NamedTemporaryFile(dir=app, delete=False) as temporary:
        staged = Path(temporary.name)
        try:
            with binary.open('rb') as source:
                shutil.copyfileobj(source, temporary)
            temporary.flush()
            os.fchmod(temporary.fileno(), 0o755)
            os.replace(staged, app / 'opterm')
        finally:
            staged.unlink(missing_ok=True)
    if not repository:
        for source in folder.iterdir():
            if source.name in ('opterm', 'install.py') or source.resolve() == app.resolve():
                continue
            if source.is_dir():
                shutil.copytree(source, app / source.name, dirs_exist_ok=True)
            elif source.is_file():
                shutil.copyfile(source, app / source.name)
    icon = folder.parent / 'assets/icons/window-icon.png' if repository else folder / 'opterm.png'
    icons = prefix / 'share/icons/hicolor/256x256/apps'
    icons.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(icon, icons / 'dev.opterm.Opterm.png')
    applications = prefix / 'share/applications'
    applications.mkdir(parents=True, exist_ok=True)
    launcher = applications / 'dev.opterm.Opterm.desktop'
    launcher.write_text('[Desktop Entry]\nType=Application\nName=Opterm\nComment=Terminal with persistent sessions\n'
                        f'Exec={desktop_quote(app / "opterm")}\nTryExec={app / "opterm"}\n'
                        'Icon=dev.opterm.Opterm\nTerminal=false\nCategories=System;TerminalEmulator;\n'
                        'StartupNotify=true\nStartupWMClass=dev.opterm.Opterm\n', encoding='utf-8')
    if shutil.which('update-desktop-database'):
        subprocess.run(['update-desktop-database', str(applications)], check=True)
    if shutil.which('gtk-update-icon-cache'):
        subprocess.run(['gtk-update-icon-cache', '-f', '-t', str(prefix / 'share/icons/hicolor')], check=True)
    print('Installed application:', app / 'opterm')
    print('Application-menu launcher:', launcher)


if __name__ == '__main__':
    main()
