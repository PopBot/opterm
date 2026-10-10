#!/usr/bin/env python3
"""Focused Linux X11 storage check, using only private temporary directories.

Requires the same Xvfb/X11 prerequisites as smoke_gui.py. No user settings,
session service, clipboard, or files are changed.
"""
import ctypes as C
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import time
import tomllib

import smoke_gui as gui


class Probe:
    """Read-only service observations plus explicit recording test requests."""
    def __init__(self, directory):
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.connect(str(directory / 'service.sock'))
        self.hello = self.call({'Hello': 1})['Hello']

    def receive(self, count):
        data = b''
        while len(data) < count:
            chunk = self.socket.recv(count - len(data))
            assert chunk, 'Session service disconnected'
            data += chunk
        return data

    def call(self, request):
        data = json.dumps(request).encode()
        self.socket.sendall(struct.pack('<I', len(data)) + data)
        length = struct.unpack('<I', self.receive(4))[0]
        assert 0 < length <= 32 * 1024 * 1024
        response = json.loads(self.receive(length))
        if isinstance(response, dict): assert 'Error' not in response, response
        return response

    def close(self):
        self.socket.close()


def main():
    gui.TARGET.mkdir(exist_ok=True)
    binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else gui.TARGET / 'debug/opterm'
    server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1100x760x24', '-nolisten', 'tcp'],
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    app = None
    dpy = None
    env = None
    with tempfile.TemporaryDirectory(prefix='opterm-storage-gui-') as temporary:
        base = Path(temporary)
        try:
            display = ':' + server.stdout.readline().decode().strip()
            env = dict(os.environ)
            env.pop('OPTERM_CONFIG_DIR', None)
            env.pop('OPTERM_CACHE_DIR', None)
            env.update(DISPLAY=display, LIBGL_ALWAYS_SOFTWARE='1', WINIT_UNIX_BACKEND='x11',
                       XDG_CONFIG_HOME=str(base / 'xdg-config'), XDG_DATA_HOME=str(base / 'xdg-data'),
                       XDG_CACHE_HOME=str(base / 'xdg-cache'), OPTERM_STATE_DIR=str(base / 'runtime'),
                       OPTERM_RECOVERY_DIR=str(base / 'recovery'))
            app = subprocess.Popen([str(binary), '--', '/bin/sh'], cwd=gui.ROOT, env=env)
            dpy = gui.wait_until(lambda: gui.X.XOpenDisplay(display.encode()))
            root = gui.X.XDefaultRootWindow(dpy)

            def windows(prefix='Opterm'):
                root_return, parent, children, count = gui.W(), gui.W(), C.POINTER(gui.W)(), C.c_uint()
                gui.X.XQueryTree(dpy, root, C.byref(root_return), C.byref(parent), C.byref(children), C.byref(count))
                result = []
                for i in range(count.value):
                    name = C.c_void_p()
                    if gui.X.XFetchName(dpy, children[i], C.byref(name)) and name:
                        title = C.string_at(name).decode(errors='replace')
                        gui.X.XFree(name)
                        if title.startswith(prefix): result.append(children[i])
                if children: gui.X.XFree(children)
                return result

            def close_window(window):
                event = gui.XEvent()
                event.message.type = 33
                event.message.display = dpy
                event.message.window = window
                event.message.message_type = gui.X.XInternAtom(dpy, b'WM_PROTOCOLS', 0)
                event.message.format = 32
                event.message.data.longs[0] = gui.X.XInternAtom(dpy, b'WM_DELETE_WINDOW', 0)
                gui.X.XSendEvent(dpy, window, 0, 0, C.byref(event)); gui.X.XFlush(dpy)

            def focus(window):
                gui.X.XMoveWindow(dpy, window, 0, 0); gui.X.XMapWindow(dpy, window)
                gui.X.XRaiseWindow(dpy, window); gui.X.XSetInputFocus(dpy, window, 1, 0)
                gui.X.XFlush(dpy); time.sleep(0.3)

            def key(symbol, pressed):
                code = gui.X.XKeysymToKeycode(dpy, symbol)
                assert code, symbol
                gui.XT.XTestFakeKeyEvent(dpy, code, pressed, 0)

            def text(value):
                for char in str(value):
                    symbol = 0xff0d if char == '\n' else ord(char)
                    code = gui.X.XKeysymToKeycode(dpy, symbol)
                    shifted = gui.X.XKeycodeToKeysym(dpy, code, 0) != symbol
                    if shifted: key(0xffe1, 1)
                    key(symbol, 1); key(symbol, 0)
                    if shifted: key(0xffe1, 0)
                gui.X.XFlush(dpy)

            def shortcut(letter):
                key(0xffe3, 1); key(0xffe1, 1); key(ord(letter), 1); key(ord(letter), 0)
                key(0xffe1, 0); key(0xffe3, 0); gui.X.XFlush(dpy); time.sleep(0.3)

            def click(x, y):
                gui.XT.XTestFakeMotionEvent(dpy, -1, x, y, 0)
                gui.XT.XTestFakeButtonEvent(dpy, 1, 1, 0); gui.XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
                gui.X.XFlush(dpy); time.sleep(0.2)

            def edit(x, y, value):
                click(x, y); key(0xffe3, 1); key(ord('a'), 1); key(ord('a'), 0); key(0xffe3, 0)
                gui.X.XFlush(dpy); text(value); time.sleep(0.2)

            def command(command, name):
                marker = base / name
                marker.unlink(missing_ok=True)
                text(command + " > '" + str(marker) + "'\n")
                gui.wait_until(lambda: marker.exists() and marker.stat().st_size > 0)
                return marker.read_text().strip()

            window = gui.wait_until(lambda: next(iter(windows()), None))
            focus(window)
            original_pid = command("export OPTERM_STORAGE=unchanged; printf '%s' $$", 'original-pid')
            runtime = Path(env['OPTERM_STATE_DIR'])
            backup = Path(env['OPTERM_RECOVERY_DIR']) / 'last-valid-settings.toml'

            def open_settings():
                focus(window); shortcut(',')
                settings = gui.wait_until(lambda: next(iter(windows('Opterm Settings')), None))
                focus(settings)
                return settings

            def recording_check():
                probe = Probe(runtime)
                assert probe.hello.get('history') is True
                snapshot = probe.call({'Create': ['/bin/sh']})['Snapshot']
                session_id = snapshot['info']['id']
                history = runtime / 'history'
                def output():
                    return b''.join(path.read_bytes() for path in sorted(history.glob('output-*.ansi')))
                def probe_command(token, name, color):
                    marker = base / name
                    marker.unlink(missing_ok=True)
                    script = "printf '\\033[" + str(color) + "m" + token + "\\033[0m\\n'; printf '%s' $$ > '" + str(marker) + "'\n"
                    assert probe.call({'Write': {'id': session_id, 'bytes': list(script.encode())}}) == 'Done'
                    gui.wait_until(lambda: marker.exists() and marker.stat().st_size > 0)
                    return marker.read_text()
                probe_pid = probe_command('REC_START_MARKER', 'record-pid', 35)
                gui.wait_until(lambda: b'\x1b[35mREC_START_MARKER\x1b[0m' in output())
                assert probe.call({'Recording': {'id': session_id, 'enabled': False}}) == 'Done'
                assert probe_command('REC_PAUSED_MARKER', 'paused-pid', 31) == probe_pid
                time.sleep(0.4)
                assert b'REC_PAUSED_MARKER' not in output(), 'Paused terminal output was recorded'
                assert probe.call({'Recording': {'id': session_id, 'enabled': True}}) == 'Done'
                assert probe_command('REC_RESUMED_MARKER', 'resumed-pid', 34) == probe_pid
                gui.wait_until(lambda: b'\x1b[34mREC_RESUMED_MARKER\x1b[0m' in output())
                assert b'REC_PAUSED_MARKER' not in output(), 'Resume backfilled paused output'
                print('Raw ANSI recording, per-session pause/resume, omitted paused bytes, and unchanged shell PID: passed')
                probe.close()

            # The native storage interaction checks follow the compact settings
            # layout. Existing shell continuity is verified after every change.
            config = base / 'xdg-config/opterm/settings/settings.toml'
            cache = base / 'xdg-cache/opterm'
            chosen = base / 'chosen-opterm'
            chosen_config = chosen / 'settings/settings.toml'
            locator = base / 'xdg-data/opterm/settings-location.json'
            def history_settings():
                open_settings(); click(435, 22)
            history_settings()
            print('Storage settings screenshot:', gui.screenshot(display, 'prototype-storage-settings-linux.png'))
            click(30, 96); click(35, 520)
            gui.wait_until(lambda: config.exists() and not windows('Opterm Settings'))
            assert tomllib.loads(config.read_text())['history']['enabled'] is False
            assert backup.read_bytes() == config.read_bytes()
            focus(window)
            assert command("printf '%s:%s' $$ \"$OPTERM_STORAGE\"", 'after-disabled-recording') == original_pid + ':unchanged'
            history_settings(); click(30, 96); click(35, 520)
            gui.wait_until(lambda: not windows('Opterm Settings'))
            assert tomllib.loads(config.read_text())['history']['enabled'] is True
            assert backup.read_bytes() == config.read_bytes()
            recording_check()

            history_settings(); edit(180, 304, chosen); click(115, 332)
            gui.wait_until(chosen_config.exists)
            assert chosen_config.read_bytes() == config.read_bytes()
            assert json.loads(locator.read_text())['root'] == str(chosen)
            assert backup.parent != chosen_config.parent
            # Cache ownership deliberately includes unrelated sentinel files:
            # the clear action must remove only Opterm's marked disposable file.
            owned = cache / 'opterm-disposable'
            cache.mkdir(parents=True, mode=0o700)
            owned.mkdir(parents=True, mode=0o700)
            (owned / '.opterm-cache').write_bytes(b'opterm disposable cache v1\n')
            disposable = owned / 'opterm-check.bin'
            disposable.write_bytes(b'disposable')
            (owned / 'unrelated.txt').write_bytes(b'keep')
            (cache / 'unrelated.txt').write_bytes(b'keep')
            protected_paths = [config, backup, chosen_config]
            protected_paths += list((runtime / 'layouts').glob('*.json'))
            protected_paths += list((runtime / 'history').glob('output-*.ansi'))
            protected = {path: path.read_bytes() for path in protected_paths}
            click(240, 332)
            gui.wait_until(lambda: not disposable.exists())
            assert all(path.read_bytes() == content for path, content in protected.items())
            assert (owned / '.opterm-cache').exists()
            assert (owned / 'unrelated.txt').read_bytes() == b'keep'
            assert (cache / 'unrelated.txt').read_bytes() == b'keep'
            click(94, 520); gui.wait_until(lambda: not windows('Opterm Settings'))
            focus(window)
            assert command("printf '%s:%s' $$ \"$OPTERM_STORAGE\"", 'after-stage-and-clear') == original_pid + ':unchanged'
            print('Native recording settings Save, staged custom folder, and cache clear preserve layouts, raw history, recovery, and the live shell: passed')

            close_window(window); assert app.wait(timeout=5) == 0
            # A distinctive valid edit proves the next launch reads the chosen
            # file rather than the old file, while recovery remains independent.
            selected_settings = chosen_config.read_bytes()
            assert b'font_size = 14.0' in selected_settings
            chosen_config.write_bytes(selected_settings.replace(b'font_size = 14.0', b'font_size = 16.0'))
            app = subprocess.Popen([str(binary)], cwd=gui.ROOT, env=env)
            window = gui.wait_until(lambda: next(iter(windows()), None)); focus(window)
            gui.wait_until(lambda: backup.read_bytes() == chosen_config.read_bytes())
            assert b'font_size = 14.0' in config.read_bytes()
            assert command("printf '%s:%s' $$ \"$OPTERM_STORAGE\"", 'after-relocation-reopen') == original_pid + ':unchanged'
            assert tomllib.loads(backup.read_text())['appearance']['font_size'] == 16.0
            new_owned = chosen / 'cache/opterm-disposable'
            new_owned.mkdir(parents=True, mode=0o700)
            (new_owned / '.opterm-cache').write_bytes(b'opterm disposable cache v1\n')
            new_disposable = new_owned / 'opterm-new-cache.bin'
            new_disposable.write_bytes(b'disposable')
            history_settings(); click(240, 332)
            gui.wait_until(lambda: not new_disposable.exists())
            assert (cache / 'unrelated.txt').read_bytes() == b'keep'
            assert backup.read_bytes() == chosen_config.read_bytes()
            print('Relocated settings/cache are used on next launch, independent backup updates, and original session reconnects without replay: passed')
            print('Relocated storage screenshot:', gui.screenshot(display, 'prototype-storage-linux.png'))

        except Exception:
            if dpy: print('Storage failure screenshot:', gui.screenshot(display, 'gui-storage-failure.png'))
            raise
        finally:
            if dpy:
                for window in windows(): close_window(window)
            if app:
                app.terminate()
                try: app.wait(timeout=3)
                except subprocess.TimeoutExpired: app.kill(); app.wait()
            if env:
                subprocess.run([str(binary), '--stop-service'], cwd=gui.ROOT, env=env,
                               timeout=5, check=True, stdout=subprocess.DEVNULL)
            if dpy: gui.X.XCloseDisplay(dpy)
            server.terminate(); server.wait(timeout=3)


if __name__ == '__main__':
    main()
