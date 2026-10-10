#!/usr/bin/env python3
"""Private nested Weston check: Opterm itself connects through native Wayland.

Requires Weston, Xvfb, and the X11 tools from smoke_gui.py. A locally extracted
Weston at target/wayland-tools/root is also supported. The nested compositor
does not use or modify the user's desktop/session service/settings.
"""
import ctypes as C
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

import smoke_gui as gui
from smoke_storage import Probe


def main():
    gui.TARGET.mkdir(exist_ok=True)
    binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else gui.TARGET / 'debug/opterm'
    local = gui.TARGET / 'wayland-tools/root'
    weston = shutil.which('weston')
    if not weston and (local / 'usr/bin/weston').exists(): weston = str(local / 'usr/bin/weston')
    if not weston: raise RuntimeError('Install Weston or extract it under target/wayland-tools/root')
    server = compositor = app = dpy = None
    env = None
    gui.TARGET.joinpath('wayland-app.log').write_text('')
    with tempfile.TemporaryDirectory(prefix='opterm-wayland-') as temporary:
        base = Path(temporary)
        try:
            server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1400x900x24', '-nolisten', 'tcp'],
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            display = ':' + server.stdout.readline().decode().strip()
            wayland_runtime = base / 'wayland'
            wayland_runtime.mkdir(mode=0o700)
            compositor_env = {**os.environ, 'DISPLAY': display, 'XDG_RUNTIME_DIR': str(wayland_runtime),
                              'LIBGL_ALWAYS_SOFTWARE': '1'}
            config = base / 'weston.ini'
            configuration = '[shell]\nlocking=false\nbackground-color=0xff171717\n'
            if Path(weston).is_relative_to(local):
                library = local / 'usr/lib/x86_64-linux-gnu'
                compositor_env['LD_LIBRARY_PATH'] = str(library / 'weston') + ':' + str(library)
                compositor_env['WESTON_MODULE_MAP'] = ';'.join(path.name + '=' + str(path)
                                                               for path in (local / 'usr/lib').rglob('*.so'))
                compositor_env['WESTON_DATA_DIR'] = str(local / 'usr/share/weston')
                configuration += 'client=' + str(local / 'usr/libexec/weston-desktop-shell') + '\n'
            config.write_text(configuration)
            with (gui.TARGET / 'wayland-compositor.log').open('w') as log:
                compositor = subprocess.Popen([weston, '--backend=x11', '--renderer=pixman', '--width=1100',
                                               '--height=760', '--socket=wayland-opterm-test', '--config=' + str(config)],
                                              env=compositor_env, stdout=log, stderr=log)
            gui.wait_until(lambda: (wayland_runtime / 'wayland-opterm-test').exists())
            dpy = gui.wait_until(lambda: gui.X.XOpenDisplay(display.encode()))
            root = gui.X.XDefaultRootWindow(dpy)
            def weston_window():
                roots, parent, children, count = gui.W(), gui.W(), C.POINTER(gui.W)(), C.c_uint()
                gui.X.XQueryTree(dpy, root, C.byref(roots), C.byref(parent), C.byref(children), C.byref(count))
                result = None
                for i in range(count.value):
                    name = C.c_void_p()
                    if gui.X.XFetchName(dpy, children[i], C.byref(name)) and name:
                        title = C.string_at(name).decode(errors='replace'); gui.X.XFree(name)
                        if 'Weston' in title: result = children[i]
                if children: gui.X.XFree(children)
                if result is None:
                    # Some Weston versions expose only _NET_WM_NAME, which
                    # XFetchName does not read. Its private backend log records
                    # the actual X11 output window ID.
                    found = re.search(r'x11 output .*window id (\d+)', (gui.TARGET / 'wayland-compositor.log').read_text())
                    if found: result = int(found.group(1))
                return result
            compositor_window = gui.wait_until(weston_window)
            gui.X.XMoveWindow(dpy, compositor_window, 0, 0)
            gui.X.XMapWindow(dpy, compositor_window); gui.X.XRaiseWindow(dpy, compositor_window)
            gui.X.XSetInputFocus(dpy, compositor_window, 1, 0); gui.X.XFlush(dpy)
            env = {**compositor_env, 'WINIT_UNIX_BACKEND': 'wayland', 'WAYLAND_DISPLAY': 'wayland-opterm-test',
                   'WAYLAND_DEBUG': 'client', 'OPTERM_STATE_DIR': str(base / 'sessions'),
                   'OPTERM_CONFIG_DIR': str(base / 'settings'), 'OPTERM_RECOVERY_DIR': str(base / 'recovery'),
                   'RUST_BACKTRACE': '1'}
            # Absence of DISPLAY prevents accidental X11 fallback by Opterm.
            env.pop('DISPLAY', None)
            pid = base / 'shell-pid'
            fixture = "export OPTERM_WAYLAND=alive; printf '\\033[32mNative Wayland\\033[0m\\nASCII | café é | 中文 日本語 | 👩‍💻 🙂\\n'; printf '%s' $$ > '" + str(pid) + "'; exec /bin/sh"
            log = (gui.TARGET / 'wayland-app.log').open('a')
            app = subprocess.Popen([str(binary), '--', '/bin/sh', '-c', fixture], cwd=gui.ROOT, env=env, stdout=log, stderr=log)
            gui.wait_until(pid.exists)
            original_pid = pid.read_text()
            runtime = Path(env['OPTERM_STATE_DIR'])
            gui.wait_until(lambda: 'wl_keyboard@' in (gui.TARGET / 'wayland-app.log').read_text()
                           and '.enter(' in (gui.TARGET / 'wayland-app.log').read_text())
            time.sleep(1.0)
            gui.XT.XTestFakeMotionEvent(dpy, -1, 300, 200, 0)
            gui.XT.XTestFakeButtonEvent(dpy, 1, 1, 0); gui.XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
            gui.X.XFlush(dpy); time.sleep(0.3)
            print('Native Wayland screenshot:', gui.screenshot(display, 'prototype-wayland-linux.png'))
            probe = Probe(runtime)
            sessions = probe.call('List')['List']; assert len(sessions) == 1
            session_id = sessions[0]['id']
            probe.call({'Attach': session_id})
            snapshot = probe.call({'View': {'id': session_id, 'since': None}})['Snapshot']
            cells = snapshot['screen']['cells']
            assert any(cell['text'] == '中' and cell['wide'] for cell in cells), 'CJK wide cells were lost'
            assert any('é' in cell['text'] for cell in cells), 'Unicode text was lost'
            assert any('é' in cell['text'] for cell in cells), 'Combining mark was lost'
            probe.close()
            def key(symbol, pressed):
                code = gui.X.XKeysymToKeycode(dpy, symbol); assert code
                gui.XT.XTestFakeKeyEvent(dpy, code, pressed, 0)
            def text(value):
                for char in value:
                    symbol = 0xff0d if char == '\n' else ord(char)
                    code = gui.X.XKeysymToKeycode(dpy, symbol)
                    shifted = gui.X.XKeycodeToKeysym(dpy, code, 0) != symbol
                    if shifted: key(0xffe1, 1)
                    key(symbol, 1); key(symbol, 0)
                    if shifted: key(0xffe1, 0)
                gui.X.XFlush(dpy)
            ready = base / 'keyboard'
            text("printf '%s:%s' $$ \"$OPTERM_WAYLAND\" > '" + str(ready) + "'\n")
            gui.wait_until(lambda: ready.exists() and ready.read_text() == original_pid + ':alive')
            gui.wait_until(lambda: list((runtime / 'layouts').glob('*.json')))
            print('Native Wayland rendering, ASCII keyboard input, Unicode/CJK/combining terminal cells: passed')
            # A desktop crash must not own or terminate the underlying shell.
            app.kill(); app.wait(timeout=5)
            app = subprocess.Popen([str(binary)], cwd=gui.ROOT, env=env, stdout=log, stderr=log)
            time.sleep(1.2)
            gui.XT.XTestFakeMotionEvent(dpy, -1, 300, 200, 0)
            gui.XT.XTestFakeButtonEvent(dpy, 1, 1, 0); gui.XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
            gui.X.XFlush(dpy); time.sleep(0.3)
            ready.unlink()
            text("printf '%s:%s' $$ \"$OPTERM_WAYLAND\" > '" + str(ready) + "'\n")
            gui.wait_until(lambda: ready.exists() and ready.read_text() == original_pid + ':alive')
            assert 'wl_display' in (gui.TARGET / 'wayland-app.log').read_text()
            print('Native Wayland desktop SIGKILL/reopen reconnects original shell PID and environment: passed')
            key(0xffe3, 1); key(0xffe1, 1); key(ord(','), 1); key(ord(','), 0)
            key(0xffe1, 0); key(0xffe3, 0); gui.X.XFlush(dpy); time.sleep(0.8)
            print('Native Wayland settings screenshot:', gui.screenshot(display, 'prototype-wayland-settings-linux.png'))
            print('Settings window creation observed visually; native IME, clipboard, screen-reader accessibility, and normal WM-close still require dedicated validation.')
            log.close()
        except Exception:
            if dpy: print('Wayland failure screenshot:', gui.screenshot(display, 'gui-wayland-failure.png'))
            raise
        finally:
            if app:
                app.terminate()
                try: app.wait(timeout=3)
                except subprocess.TimeoutExpired: app.kill(); app.wait()
            if env:
                subprocess.run([str(binary), '--stop-service'], cwd=gui.ROOT, env=env,
                               timeout=5, check=True, stdout=subprocess.DEVNULL)
            if compositor: compositor.terminate(); compositor.wait(timeout=3)
            if dpy: gui.X.XCloseDisplay(dpy)
            if server: server.terminate(); server.wait(timeout=3)


if __name__ == '__main__':
    main()
