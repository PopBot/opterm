#!/usr/bin/env python3
"""Focused Linux X11 check: input, clipboard, and real close/crash continuity.

Uses a private Xvfb display. Requires Xvfb, xwd, libX11 and libXtst.
Does not touch the user's desktop or require Python packages.
"""
import ctypes as C
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import tempfile
import zlib
import tomllib
import json
import socket
import re

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / 'target'
X = C.CDLL('libX11.so.6')
XT = C.CDLL('libXtst.so.6')
D = C.c_void_p
W = C.c_ulong
X.XOpenDisplay.argtypes = [C.c_char_p]
X.XOpenDisplay.restype = D
X.XDefaultRootWindow.argtypes = [D]
X.XDefaultRootWindow.restype = W
X.XQueryTree.argtypes = [D, W, C.POINTER(W), C.POINTER(W), C.POINTER(C.POINTER(W)), C.POINTER(C.c_uint)]
X.XFetchName.argtypes = [D, W, C.POINTER(C.c_void_p)]
X.XFree.argtypes = [C.c_void_p]
X.XKeysymToKeycode.argtypes = [D, W]
X.XKeysymToKeycode.restype = C.c_uint
X.XKeycodeToKeysym.argtypes = [D, C.c_uint, C.c_int]
X.XKeycodeToKeysym.restype = W
X.XSetInputFocus.argtypes = [D, W, C.c_int, W]
X.XMapWindow.argtypes = [D, W]
X.XRaiseWindow.argtypes = [D, W]
X.XSync.argtypes = [D, C.c_int]
X.XMoveWindow.argtypes = [D, W, C.c_int, C.c_int]
X.XResizeWindow.argtypes = [D, W, C.c_uint, C.c_uint]
X.XFlush.argtypes = [D]
X.XCloseDisplay.argtypes = [D]
X.XInternAtom.argtypes = [D, C.c_char_p, C.c_int]
X.XInternAtom.restype = W
class MessageData(C.Union):
    _fields_ = [('longs', C.c_long * 5), ('bytes', C.c_char * 20)]
class ClientMessage(C.Structure):
    _fields_ = [('type', C.c_int), ('serial', W), ('send_event', C.c_int),
                ('display', D), ('window', W), ('message_type', W),
                ('format', C.c_int), ('data', MessageData)]
class XEvent(C.Union):
    _fields_ = [('message', ClientMessage), ('padding', C.c_long * 24)]
X.XSendEvent.argtypes = [D, W, C.c_int, C.c_long, C.POINTER(XEvent)]
XT.XTestFakeKeyEvent.argtypes = [D, C.c_uint, C.c_int, W]
XT.XTestFakeMotionEvent.argtypes = [D, C.c_int, C.c_int, C.c_int, W]
XT.XTestFakeButtonEvent.argtypes = [D, C.c_uint, C.c_int, W]


def wait_until(check, seconds=12):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.05)
    raise RuntimeError('GUI check timed out')


def screenshot(display, filename="prototype-linux.png"):
    data = subprocess.check_output(['xwd', '-display', display, '-root', '-silent'])
    h = struct.unpack('>25I', data[:100])
    header, _, _, depth, width, height, _, byte_order, _, _, _, bits, stride, _, rm, gm, bm, _, _, colors, *_ = h
    assert depth == 24 and bits in (24, 32), (depth, bits)
    pixel_bytes = bits // 8
    pixels = data[header + colors * 12:]
    shifts = [(mask & -mask).bit_length() - 1 for mask in (rm, gm, bm)]
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            offset = y * stride + x * pixel_bytes
            pixel = int.from_bytes(pixels[offset:offset + pixel_bytes], 'little' if byte_order == 0 else 'big')
            rows.extend((pixel >> shift) & 255 for shift in shifts)
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>2I5B', width, height, 8, 2, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')
    path = TARGET / filename
    path.write_bytes(png)
    return path


def main():
    TARGET.mkdir(exist_ok=True)
    marker = TARGET / 'gui-input.txt'
    marker.unlink(missing_ok=True)
    server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1100x760x24', '-nolisten', 'tcp'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    state = tempfile.TemporaryDirectory(prefix='opterm-gui-')
    app = None
    dpy = None
    env = None
    controls_only = '--controls-only' in sys.argv
    arguments = [arg for arg in sys.argv[1:] if arg != '--controls-only']
    binary = Path(arguments[0]).resolve() if arguments else TARGET / 'debug/opterm'
    try:
        display = ':' + server.stdout.readline().decode().strip()
        env = {**os.environ, 'DISPLAY': display, 'LIBGL_ALWAYS_SOFTWARE': '1', 'WINIT_UNIX_BACKEND': 'x11', 'OPTERM_STATE_DIR': state.name + '/runtime', 'OPTERM_CONFIG_DIR': state.name + '/settings', 'OPTERM_RECOVERY_DIR': state.name + '/recovery'}
        app = subprocess.Popen([str(binary), '--', '/bin/sh'], cwd=ROOT, env=env)
        dpy = wait_until(lambda: X.XOpenDisplay(display.encode()))
        root = X.XDefaultRootWindow(dpy)
        def find_windows(prefix="Opterm"):
            root_return, parent, children, count = W(), W(), C.POINTER(W)(), C.c_uint()
            X.XQueryTree(dpy, root, C.byref(root_return), C.byref(parent), C.byref(children), C.byref(count))
            found = []
            for i in range(count.value):
                name = C.c_void_p()
                if X.XFetchName(dpy, children[i], C.byref(name)) and name:
                    title = C.string_at(name).decode(errors='replace')
                    X.XFree(name)
                    if title.startswith(prefix):
                        found.append(children[i])
            if children:
                X.XFree(children)
            return found
        def find_window():
            windows = find_windows()
            return windows[-1] if windows else None
        def close_window(window):
            event = XEvent()
            event.message.type = 33
            event.message.display = dpy
            event.message.window = window
            event.message.message_type = X.XInternAtom(dpy, b'WM_PROTOCOLS', 0)
            event.message.format = 32
            event.message.data.longs[0] = X.XInternAtom(dpy, b'WM_DELETE_WINDOW', 0)
            X.XSendEvent(dpy, window, 0, 0, C.byref(event))
            X.XFlush(dpy)
        window = wait_until(find_window)
        X.XMapWindow(dpy, window)
        X.XSync(dpy, 0)
        time.sleep(0.3)
        X.XSetInputFocus(dpy, window, 1, 0)
        X.XFlush(dpy)
        time.sleep(0.5)
        def key(symbol, pressed):
            code = X.XKeysymToKeycode(dpy, symbol)
            assert code, symbol
            XT.XTestFakeKeyEvent(dpy, code, pressed, 0)
        def type_text(text):
            for char in text:
                symbol = 0xff0d if char == '\n' else ord(char)
                code = X.XKeysymToKeycode(dpy, symbol)
                shifted = X.XKeycodeToKeysym(dpy, code, 0) != symbol
                if shifted:
                    key(0xffe1, 1)
                key(symbol, 1)
                key(symbol, 0)
                if shifted:
                    key(0xffe1, 0)
            X.XFlush(dpy)
        def reopen():
            process = subprocess.Popen([str(binary)], cwd=ROOT, env=env)
            new_window = wait_until(find_window)
            X.XMapWindow(dpy, new_window)
            X.XSync(dpy, 0)
            time.sleep(0.3)
            X.XSetInputFocus(dpy, new_window, 1, 0)
            X.XFlush(dpy)
            time.sleep(0.5)
            return process, new_window
        def shortcut(symbol, alt=False):
            key(0xffe3, 1); key(0xffe1, 1)
            if alt: key(0xffe9, 1)
            key(symbol, 1); key(symbol, 0)
            if alt: key(0xffe9, 0)
            key(0xffe1, 0); key(0xffe3, 0)
            X.XFlush(dpy); time.sleep(0.35)
        def click(x, y):
            XT.XTestFakeMotionEvent(dpy, -1, x, y, 0)
            XT.XTestFakeButtonEvent(dpy, 1, 1, 0)
            XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
            X.XFlush(dpy); time.sleep(0.2)
        def command_file(command, name):
            path = Path(state.name) / name
            path.unlink(missing_ok=True)
            type_text(command + " > '" + str(path) + "'\n")
            wait_until(lambda: path.exists() and path.stat().st_size > 0)
            return path.read_text().strip()

        def controls():
            nonlocal app, window, env
            # Everyday controls use their own service and settings so their tab
            # transfers cannot change the continuity fixtures above.
            for current in find_windows(): close_window(current)
            wait_until(lambda: not find_windows())
            app.wait(timeout=5)
            continuity_env = env
            env = {**env, 'OPTERM_STATE_DIR': state.name + '/controls/runtime',
                   'OPTERM_CONFIG_DIR': state.name + '/controls/settings',
                   'OPTERM_RECOVERY_DIR': state.name + '/controls/recovery'}
            try:
                app = subprocess.Popen([str(binary), '--', '/bin/sh'], cwd=ROOT, env=env)
                window = wait_until(find_window)
                X.XMoveWindow(dpy, window, 0, 0); X.XResizeWindow(dpy, window, 1000, 680)
                X.XMapWindow(dpy, window); X.XSetInputFocus(dpy, window, 1, 0); X.XFlush(dpy)
                time.sleep(0.5)
                alpha_pid = command_file("export TAB=alpha; printf '%s' $$", 'controls-alpha')
                shortcut(ord('t'))
                beta_left_pid = command_file("export TAB=beta-left; printf '%s' $$", 'controls-beta-left')
                shortcut(ord('e'))
                beta_right_pid = command_file("export TAB=beta-right; printf '%s' $$", 'controls-beta-right')
                shortcut(ord('t'))
                gamma_pid = command_file("export TAB=gamma; printf '%s' $$", 'controls-gamma')
                assert len({alpha_pid, beta_left_pid, beta_right_pid, gamma_pid}) == 4
                close_window(window); assert app.wait(timeout=5) == 0
                controls_layouts = Path(env['OPTERM_STATE_DIR']) / 'layouts'
                source_path = next(controls_layouts.glob('*.json'))
                source_doc = json.loads(source_path.read_text())
                assert len(source_doc['tabs']) == 3
                for tab, name in zip(source_doc['tabs'], ['Alpha', 'Beta', 'Gamma']):
                    tab['name'] = name
                source_doc['size'] = [1000, 680]; source_doc['position'] = [0, 0]
                source_path.write_text(json.dumps(source_doc))
                app, window = reopen()
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-restored-gamma') == gamma_pid + ':gamma'
                # Phase-two interaction assertions are below; all checks use saved
                # session references and actual shell PIDs rather than new launches.
                search_ready = Path(state.name) / 'controls-search-ready'
                type_text("printf '\\033[2J\\033[H'; python3 -c 'for i in range(180): print((\" \" * {3:0,60:10,150:20}[i] + (\"nee\" + \"dle.[*\")) if i in (3,60,150) else \"filler \" + str(i))'; printf READY > '" + str(search_ready) + "'\n")
                wait_until(search_ready.exists)
                def selection_left():
                    data = subprocess.check_output(['xwd', '-display', display, '-root', '-silent'])
                    h = struct.unpack('>25I', data[:100])
                    pixel_bytes = h[11] // 8
                    start = h[0] + h[19] * 12
                    shifts = [(mask & -mask).bit_length() - 1 for mask in h[14:17]]
                    found = []
                    for y in range(65, 660, 3):
                        for x in range(6, 340, 2):
                            offset = start + y * h[12] + x * pixel_bytes
                            pixel = int.from_bytes(data[offset:offset + pixel_bytes], 'little' if h[7] == 0 else 'big')
                            if tuple((pixel >> shift) & 255 for shift in shifts) == (65, 83, 112):
                                found.append(x)
                    return min(found) if found else None
                shortcut(ord('f'))
                type_text('needle.[*')
                first_match = wait_until(lambda: selection_left() is not None and selection_left())
                key(0xff0d, 1); key(0xff0d, 0); X.XFlush(dpy)
                next_match = wait_until(lambda: selection_left() is not None and selection_left() != first_match and selection_left())
                assert next_match != first_match
                key(0xffe1, 1); key(0xff0d, 1); key(0xff0d, 0); key(0xffe1, 0); X.XFlush(dpy)
                wait_until(lambda: selection_left() == first_match)
                key(0xffe3, 1); key(ord('a'), 1); key(ord('a'), 0); key(0xffe3, 0); X.XFlush(dpy)
                type_text('needle.+')
                wait_until(lambda: selection_left() is None)
                key(0xff1b, 1); key(0xff1b, 0); X.XFlush(dpy); time.sleep(0.3)
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-after-search') == gamma_pid + ':gamma'
                print('Compact literal scrollback search, Enter/Shift+Enter navigation, and Escape return to the same shell: passed')
                def focus_window(current):
                    X.XMoveWindow(dpy, current, 0, 0); X.XRaiseWindow(dpy, current)
                    X.XSetInputFocus(dpy, current, 1, 0); X.XFlush(dpy); time.sleep(0.3)
                def drag(x, y, target_x, target_y):
                    XT.XTestFakeMotionEvent(dpy, -1, x, y, 0)
                    XT.XTestFakeButtonEvent(dpy, 1, 1, 0); X.XFlush(dpy); time.sleep(0.15)
                    XT.XTestFakeMotionEvent(dpy, -1, x + 12, y, 0); X.XFlush(dpy); time.sleep(0.15)
                    XT.XTestFakeMotionEvent(dpy, -1, target_x, target_y, 0); X.XFlush(dpy); time.sleep(0.2)
                    XT.XTestFakeButtonEvent(dpy, 1, 0, 0); X.XFlush(dpy); time.sleep(0.2)
                def names(path):
                    return [tab['name'] for tab in json.loads(path.read_text())['tabs']]
                focus_window(window)
                drag(135, 15, 20, 15)  # Gamma before Alpha.
                wait_until(lambda: names(source_path) == ['Gamma', 'Alpha', 'Beta'])
                reordered = json.loads(source_path.read_text())
                assert reordered['tabs'][2] == source_doc['tabs'][1], 'Reordering changed the split tab'
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-reordered-gamma') == gamma_pid + ':gamma'
                print('Tab drag reorders saved tabs while preserving active shell and split metadata: passed')
                before = set(find_windows())
                drag(145, 15, 1050, 100)  # Beta outside the source window.
                receiving_window = wait_until(lambda: next(iter(set(find_windows()) - before), None))
                wait_until(lambda: names(source_path) == ['Gamma', 'Alpha'])
                receiver_path = wait_until(lambda: next((path for path in controls_layouts.glob('*.json') if path != source_path), None))
                wait_until(lambda: names(receiver_path) == ['Beta'])
                assert json.loads(receiver_path.read_text())['tabs'][0] == source_doc['tabs'][1]
                focus_window(receiving_window)
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-moved-beta-right') == beta_right_pid + ':beta-right'
                shortcut(0xff51)
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-moved-beta-left') == beta_left_pid + ':beta-left'
                shortcut(0xff53)
                # Move Alpha through the existing-window context-menu destination.
                focus_window(window)
                XT.XTestFakeMotionEvent(dpy, -1, 95, 15, 0)
                XT.XTestFakeButtonEvent(dpy, 3, 1, 0); XT.XTestFakeButtonEvent(dpy, 3, 0, 0)
                X.XFlush(dpy); time.sleep(0.25)
                click(135, 45)  # Second item: Move to Beta.
                wait_until(lambda: names(source_path) == ['Gamma'])
                wait_until(lambda: names(receiver_path) == ['Beta', 'Alpha'])
                received = json.loads(receiver_path.read_text())
                assert received['tabs'][0] == source_doc['tabs'][1]
                assert received['tabs'][1] == source_doc['tabs'][0]
                focus_window(receiving_window)
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-moved-alpha') == alpha_pid + ':alpha'
                # The source window still owns Gamma; transfers launch no replacement
                # shell and do not rerun initialization commands in either split.
                focus_window(window)
                assert command_file("printf '%s:%s' $$ \"$TAB\"", 'controls-source-gamma') == gamma_pid + ':gamma'
                with socket.socket(socket.AF_UNIX) as probe:
                    probe.connect(str(Path(env['OPTERM_STATE_DIR']) / 'service.sock'))
                    def call_controls(request):
                        data = json.dumps(request).encode(); probe.sendall(struct.pack('<I', len(data)) + data)
                        def receive(count):
                            data = b''
                            while len(data) < count:
                                chunk = probe.recv(count - len(data)); assert chunk; data += chunk
                            return data
                        length = struct.unpack('<I', receive(4))[0]
                        return json.loads(receive(length))
                    call_controls({'Hello': 1})
                    assert len(call_controls('List')['List']) == 4, 'Moving a tab launched replacement shells'
                print('Drag to new window and move to existing window preserve names, splits, focus, original PIDs, and exactly four shells: passed')
                print('Everyday controls screenshot:', screenshot(display, 'prototype-controls-linux.png'))
            except Exception:
                print('Controls failure screenshot:', screenshot(display, 'gui-controls-failure.png'))
                raise
            finally:
                for current in find_windows(): close_window(current)
                wait_until(lambda: not find_windows())
                if app.poll() is None: app.wait(timeout=5)
                subprocess.run([str(binary), '--stop-service'], cwd=ROOT, env=env,
                               timeout=5, check=True, stdout=subprocess.DEVNULL)
                env = continuity_env

        if controls_only:
            controls()
            return

        # New tab using the accepted Linux binding.
        key(0xffe3, 1); key(0xffe1, 1); key(ord('t'), 1); key(ord('t'), 0); key(0xffe1, 0); key(0xffe3, 0)
        X.XFlush(dpy)
        time.sleep(0.5)
        # Interrupt must reach the shell rather than copying the screen.
        type_text('sleep 30\n')
        time.sleep(0.2)
        key(0xffe3, 1); key(ord('c'), 1); key(ord('c'), 0); key(0xffe3, 0)
        X.XFlush(dpy)
        time.sleep(0.2)
        X.XResizeWindow(dpy, window, 1000, 680)
        X.XFlush(dpy)
        type_text("printf '\\033[2J\\033[H\\033[32mOpterm prototype\\033[0m\\n\\nA real shell in a thin native window.\\n\\n'; printf 'ANSI colors: \\033[31mred \\033[32mgreen \\033[34mblue\\033[0m\\n'; printf 'Terminal size: '; stty size; printf '\\n'; printf GUI_INPUT_OK > '" + str(marker) + "'\n")
        try:
            wait_until(lambda: marker.exists() and marker.read_text() == 'GUI_INPUT_OK')
        except RuntimeError:
            print('Failure screenshot:', screenshot(display))
            raise
        time.sleep(0.3)
        print('GUI input, new tab, Ctrl+C, and shell command: passed')
        print('Screenshot:', screenshot(display))
        # Round-trip a selected terminal word through the native clipboard.
        clipboard_marker = TARGET / 'gui-clipboard.txt'
        clipboard_marker.unlink(missing_ok=True)
        type_text("printf '\\033[2J\\033[Hprintf\\n'\n")
        time.sleep(0.3)
        XT.XTestFakeMotionEvent(dpy, -1, 7, 43, 0)
        XT.XTestFakeButtonEvent(dpy, 1, 1, 0)
        X.XFlush(dpy)
        time.sleep(0.15)
        XT.XTestFakeMotionEvent(dpy, -1, 53, 43, 0)
        X.XFlush(dpy)
        time.sleep(0.15)
        XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
        X.XFlush(dpy)
        for letter in ('c', 'v'):
            key(0xffe3, 1); key(0xffe1, 1); key(ord(letter), 1); key(ord(letter), 0); key(0xffe1, 0); key(0xffe3, 0)
            X.XFlush(dpy)
            time.sleep(0.25)
        type_text(" CLIPBOARD_OK > '" + str(clipboard_marker) + "'\n")
        try:
            wait_until(lambda: clipboard_marker.exists() and clipboard_marker.read_text() == 'CLIPBOARD_OK')
        except RuntimeError:
            screenshot(display)
            raise
        print('Native selection, copy, and paste: passed')
        # Inspect bytes at a real raw PTY, not a shell's line editor. The helper
        # restores terminal modes before returning to the same persistent shell.
        capture_script = Path(state.name) / 'capture-input.py'
        capture_script.write_text("""import os, select, sys, termios, time, tty
from pathlib import Path
ready, result, modes = sys.argv[1:]
saved = termios.tcgetattr(0)
data = bytearray()
try:
    tty.setraw(0)
    os.write(1, modes.encode())
    Path(ready).write_text('ready')
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        if select.select([0], [], [], 0.5 if data else 0.1)[0]:
            data.extend(os.read(0, 65536))
        elif data:
            break
    Path(result).write_bytes(data)
finally:
    os.write(1, b'\\x1b[?2004l\\x1b[?1000l\\x1b[?1002l\\x1b[?1003l\\x1b[?1006l')
    termios.tcsetattr(0, termios.TCSANOW, saved)
""")
        def capture_input(name, modes, send):
            ready = Path(state.name) / (name + '-ready')
            result = Path(state.name) / (name + '-bytes')
            # Expand fixed escaped modes in the shell so control characters
            # never enter the command line being edited interactively.
            command = "python3 '" + str(capture_script) + "' '" + str(ready) + "' '" + str(result) + "' "
            command += '"$(printf \'%b\' \'' + modes.replace('\x1b', '\\033') + '\')"\n'
            type_text(command)
            wait_until(ready.exists)
            time.sleep(0.3)
            send()
            wait_until(result.exists)
            time.sleep(0.2)
            return result.read_bytes()
        # Native clipboard selection ownership needs xclip only for this test;
        # the application itself uses its normal platform clipboard integration.
        import shutil
        if shutil.which('xclip'):
            paste = b'alpha\r\nbeta\ngamma\tend'
            clipboard = subprocess.Popen(['xclip', '-selection', 'clipboard', '-quiet'],
                                         env=env, stdin=subprocess.PIPE,
                                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                clipboard.stdin.write(paste); clipboard.stdin.close()
                time.sleep(0.2)
                def paste_input():
                    key(0xffe3, 1); key(0xffe1, 1); key(ord('v'), 1); key(ord('v'), 0)
                    key(0xffe1, 0); key(0xffe3, 0); X.XFlush(dpy)
                actual = capture_input('bracketed-paste', '\x1b[?2004h', paste_input)
                assert actual == b'\x1b[200~' + paste + b'\x1b[201~', actual
            finally:
                clipboard.terminate(); clipboard.wait(timeout=3)
            print('Native bracketed paste preserves exact CRLF, LF, and tab bytes: passed')
        else:
            print('Exact native clipboard-byte check skipped: xclip is unavailable')
        def special_keys():
            for symbol in (0xffbe, 0xffc9):  # F1, F12.
                key(symbol, 1); key(symbol, 0)
            key(0xffe3, 1); key(0xff53, 1); key(0xff53, 0); key(0xffe3, 0)
            key(0xffe1, 1); key(0xff09, 1); key(0xff09, 0); key(0xffe1, 0)
            X.XFlush(dpy)
        actual = capture_input('special-keys', '', special_keys)
        assert actual == b'\x1bOP\x1b[24~\x1b[1;5C\x1b[Z', actual
        print('Native F1, F12, Ctrl+Right, and Shift+Tab sequences: passed')
        def mouse_input():
            XT.XTestFakeMotionEvent(dpy, -1, 80, 90, 0)
            XT.XTestFakeButtonEvent(dpy, 1, 1, 0); XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
            XT.XTestFakeButtonEvent(dpy, 4, 1, 0); XT.XTestFakeButtonEvent(dpy, 4, 0, 0)
            X.XFlush(dpy)
        actual = capture_input('mouse-reporting', '\x1b[?1000h\x1b[?1006h', mouse_input)
        reports = re.findall(rb'\x1b\[<(\d+);(\d+);(\d+)([Mm])', actual)
        assert 3 <= len(reports) <= 8, actual
        assert [(code, ending) for code, _, _, ending in reports[:2]] == [(b'0', b'M'), (b'0', b'm')], actual
        # Native wheel distance can become several terminal scroll steps.
        assert all(code == b'64' and ending == b'M' for code, _, _, ending in reports[2:]), actual
        assert all(int(col) > 0 and int(row) > 0 for _, col, row, _ in reports), actual
        assert len({(col, row) for _, col, row, _ in reports}) == 1, actual
        print('Native SGR mouse press/release and wheel reporting: passed')
        # Settings live in a separate native window. All preferences paths are isolated.
        def open_settings():
            key(0xffe3, 1); key(0xffe1, 1); key(ord(','), 1); key(ord(','), 0); key(0xffe1, 0); key(0xffe3, 0)
            X.XFlush(dpy)
            settings = wait_until(lambda: next(iter(find_windows("Opterm Settings")), None))
            X.XMoveWindow(dpy, settings, 0, 0)
            X.XMapWindow(dpy, settings)
            X.XSetInputFocus(dpy, settings, 1, 0)
            X.XFlush(dpy)
            time.sleep(0.4)
            return settings
        def field(value):
            click(175, 118)
            key(0xffe3, 1); key(ord('a'), 1); key(ord('a'), 0); key(0xffe3, 0)
            X.XFlush(dpy); time.sleep(0.15)
            type_text(str(value))
            time.sleep(0.15)
            type_text('\n')
            time.sleep(0.2)
        def terminal_focus():
            X.XSetInputFocus(dpy, window, 1, 0); X.XFlush(dpy); time.sleep(0.2)
        config = Path(env['OPTERM_CONFIG_DIR']) / 'settings.toml'
        backup = Path(env['OPTERM_RECOVERY_DIR']) / 'last-valid-settings.toml'
        before_settings = Path(state.name) / 'before-settings'
        type_text("printf '%s:' $$ > '" + str(before_settings) + "'; stty size >> '" + str(before_settings) + "'\n")
        wait_until(before_settings.exists)
        settings_window = open_settings()
        field(18)
        click(94, 520)  # Cancel
        wait_until(lambda: not find_windows("Opterm Settings"))
        assert not config.exists() and not backup.exists(), 'Cancel persisted the draft'
        terminal_focus()
        settings_window = open_settings()
        field(16)
        click(35, 520)  # Save
        wait_until(lambda: config.exists() and backup.exists())
        wait_until(lambda: not find_windows("Opterm Settings"))
        assert 'font_size = 16.0' in config.read_text(), config.read_text()
        assert config.read_bytes() == backup.read_bytes()
        good = backup.read_bytes()
        terminal_focus()
        after_settings = Path(state.name) / 'after-settings'
        type_text("printf '%s:' $$ > '" + str(after_settings) + "'; stty size >> '" + str(after_settings) + "'\n")
        wait_until(after_settings.exists)
        before_pid, before_size = before_settings.read_text().split(':')
        after_pid, after_size = after_settings.read_text().split(':')
        assert before_pid == after_pid, 'Appearance save restarted the shell'
        assert before_size != after_size, 'Saved font size did not resize the terminal'
        settings_window = open_settings()
        field(18)
        config.write_bytes(good.replace(b'font_size = 16.0', b'font_size = 15.0'))
        time.sleep(1.3)
        click(35, 520)  # Save must report conflict, preserving both draft and external file.
        assert find_windows("Opterm Settings"), 'Conflicting save closed the draft'
        assert 'font_size = 15.0' in config.read_text()
        click(350, 520)  # Explicit Replace file with my draft.
        wait_until(lambda: not find_windows("Opterm Settings"))
        assert 'font_size = 18.0' in config.read_text()
        assert backup.read_bytes() == config.read_bytes()
        good = backup.read_bytes()
        terminal_focus()
        # An unchanged draft must not bypass conflict protection on a second Save.
        settings_window = open_settings()
        config.write_text('invalid = [')
        time.sleep(1.3)
        click(35, 520); click(35, 520)
        assert config.read_text() == 'invalid = [' and backup.read_bytes() == good
        click(94, 520)
        wait_until(lambda: not find_windows("Opterm Settings"))
        terminal_focus(); settings_window = open_settings()
        # Invalid live edits leave both the last-valid backup and current appearance intact.
        settings_window = wait_until(lambda: next(iter(find_windows("Opterm Settings")), None))
        X.XMoveWindow(dpy, settings_window, 0, 0)
        X.XSetInputFocus(dpy, settings_window, 1, 0); X.XFlush(dpy); time.sleep(0.3)
        assert backup.read_bytes() == good
        click(100, 496)  # Load recovery into draft.
        assert config.read_text() == 'invalid = [', 'Restore silently overwrote the file'
        click(35, 520)
        wait_until(lambda: config.read_bytes() == good)
        wait_until(lambda: not find_windows("Opterm Settings"))
        # A failed Save keeps the native editor open; repairing the destination permits retry.
        terminal_focus(); settings_window = open_settings(); field(19)
        backup.parent.chmod(0o500)
        click(35, 520)
        assert find_windows("Opterm Settings") and backup.read_bytes() == good and config.read_bytes() == good
        backup.parent.chmod(0o700)
        click(35, 520)
        wait_until(lambda: 'font_size = 19.0' in config.read_text())
        wait_until(lambda: not find_windows("Opterm Settings"))
        # Return to accepted defaults so subsequent selection/split geometry checks stay meaningful.
        terminal_focus(); settings_window = open_settings(); field(14); click(35, 520)
        wait_until(lambda: not find_windows("Opterm Settings"))
        assert 'font_size = 14.0' in config.read_text()
        # Reassign one application shortcut through the existing live-file and
        # recovery path. Its former chord must stop creating tabs immediately.
        terminal_focus()
        original_shortcuts = config.read_bytes()
        preferences = tomllib.loads(original_shortcuts.decode())
        assert preferences['copy_on_select'] is False
        assert preferences['shortcuts']['new_tab'] == 'Ctrl+Shift+T'
        assert preferences['shortcuts']['next_tab'] == ''
        assert preferences['shortcuts']['previous_tab'] == ''
        custom_shortcuts = original_shortcuts.replace(b'new_tab = "Ctrl+Shift+T"', b'new_tab = "Ctrl+Shift+Y"')
        assert custom_shortcuts != original_shortcuts
        config.write_bytes(custom_shortcuts)
        wait_until(lambda: backup.read_bytes() == custom_shortcuts)
        layout_path = wait_until(lambda: next(iter((Path(env['OPTERM_STATE_DIR']) / 'layouts').glob('*.json')), None))
        def tab_count():
            return len(json.loads(layout_path.read_text())['tabs'])
        def app_chord(letter):
            key(0xffe3, 1); key(0xffe1, 1); key(ord(letter), 1); key(ord(letter), 0)
            key(0xffe1, 0); key(0xffe3, 0); X.XFlush(dpy)
        count = tab_count()
        app_chord('t'); time.sleep(1.3)
        assert tab_count() == count, 'The former New tab shortcut was still active'
        # The former chord now belongs to the terminal; clear any shell input.
        key(0xffe3, 1); key(ord('c'), 1); key(ord('c'), 0); key(0xffe3, 0); X.XFlush(dpy)
        app_chord('y')
        wait_until(lambda: tab_count() == count + 1)
        app_chord('w')
        wait_until(lambda: tab_count() == count)
        config.write_bytes(original_shortcuts)
        wait_until(lambda: backup.read_bytes() == original_shortcuts)
        print('Custom New tab binding applies live, former binding is disabled, and settings recovery tracks the change: passed')
        terminal_focus(); settings_window = open_settings()
        print('Settings screenshot:', screenshot(display, 'prototype-settings-linux.png'))
        # Compact profile and theme editors, using real native controls.
        def edit(x, y, value):
            click(x, y)
            key(0xffe3, 1); key(ord('a'), 1); key(ord('a'), 0); key(0xffe3, 0)
            X.XFlush(dpy); time.sleep(0.1); type_text(str(value)); time.sleep(0.15)
            type_text('\n'); time.sleep(0.15)
        def app_shortcut(symbol):
            key(0xffe3, 1); key(0xffe1, 1); key(symbol, 1); key(symbol, 0); key(0xffe1, 0); key(0xffe3, 0)
            X.XFlush(dpy); time.sleep(0.4)
        def profile_command(command, name):
            path = Path(state.name) / name
            path.unlink(missing_ok=True)
            type_text(command + " > '" + str(path) + "'\n")
            wait_until(lambda: path.exists() and path.stat().st_size > 0)
            return path.read_text().strip()
        def screen_pixel(x, y):
            data = subprocess.check_output(['xwd', '-display', display, '-root', '-silent'])
            h = struct.unpack('>25I', data[:100])
            pixel_bytes = h[11] // 8
            offset = h[0] + h[19] * 12 + y * h[12] + x * pixel_bytes
            pixel = int.from_bytes(data[offset:offset + pixel_bytes], 'little' if h[7] == 0 else 'big')
            return tuple((pixel >> ((mask & -mask).bit_length() - 1)) & 255 for mask in h[14:17])
        profile_folder = Path(state.name) / 'profile folder'
        profile_folder.mkdir()
        click(285, 22); time.sleep(0.3)
        click(230, 89); time.sleep(0.3)  # Add profile.
        edit(230, 166, 'GUI shell'); edit(230, 193, '/bin/sh'); edit(230, 219, profile_folder)
        click(160, 247); click(160, 297)  # Light theme.
        click(65, 140)  # Use as default.
        print('Profiles screenshot:', screenshot(display, 'prototype-profiles-linux.png'))
        click(35, 520); wait_until(lambda: not find_windows('Opterm Settings'))
        data = tomllib.loads(config.read_text())
        assert data['profiles'][0]['name'] == 'GUI shell' and data['profiles'][0]['theme'] == 'light', data
        assert data['default_profile'] == data['profiles'][0]['id']
        assert backup.read_bytes() == config.read_bytes()
        terminal_focus(); app_shortcut(ord('t'))
        assert profile_command("printf '%s' \"$PWD\"", 'profile-cwd') == str(profile_folder)
        light_pid = profile_command("printf '%s' $$", 'profile-light-pid')
        wait_until(lambda: screen_pixel(2, 32) == (248, 248, 249))
        print('Light terminal screenshot:', screenshot(display, 'prototype-light-linux.png'))
        settings_window = open_settings(); click(285, 22); click(180, 62); time.sleep(0.3)
        click(295, 89); time.sleep(0.3)  # Duplicate built-in Dark.
        edit(170, 114, 'GUI Theme')
        print('Themes screenshot:', screenshot(display, 'prototype-themes-linux.png'))
        click(35, 520); wait_until(lambda: not find_windows('Opterm Settings'))
        theme_id = tomllib.loads(config.read_text())['themes'][0]['id']
        config.write_text(config.read_text().replace('theme = "light"', 'theme = "' + theme_id + '"'))
        time.sleep(1.3); terminal_focus()
        # Existing profile launches retain the selected theme; new tabs use the edited preset.
        assert profile_command("printf '%s' $$", 'profile-light-retained') == light_pid
        assert screen_pixel(2, 32) == (248, 248, 249)
        app_shortcut(ord('t'))
        custom_pid = profile_command("printf '%s' $$", 'profile-custom-pid')
        assert custom_pid != light_pid
        config.write_text(config.read_text().replace('#1f1f21', '#203040'))
        wait_until(lambda: screen_pixel(2, 32) == (32, 48, 64))
        assert profile_command("printf '%s' $$", 'profile-theme-kept-pid') == custom_pid
        app_shortcut(ord('e'))  # Default-profile split captures the same folder/theme, with its own process.
        split_pid = profile_command("printf '%s' $$", 'profile-split-pid')
        assert split_pid not in (custom_pid, light_pid)
        assert profile_command("printf '%s' \"$PWD\"", 'profile-split-cwd') == str(profile_folder)
        settings_window = open_settings(); click(285, 22); click(180, 62); time.sleep(0.3)
        click(150, 89); click(50, 160); time.sleep(0.3)  # Select custom theme.
        click(365, 89); click(35, 520)  # Delete theme then Save.
        wait_until(lambda: not find_windows('Opterm Settings'))
        terminal_focus(); wait_until(lambda: screen_pixel(2, 32) == (31, 31, 33))
        assert profile_command("printf '%s' $$", 'profile-after-theme-delete') == split_pid
        settings_window = open_settings(); click(285, 22); time.sleep(0.2)
        click(150, 89); click(50, 137); time.sleep(0.2)  # Select GUI shell profile.
        click(365, 89); click(35, 520)  # Delete default profile, reverting to built-in Shell.
        wait_until(lambda: not find_windows('Opterm Settings'))
        terminal_focus()
        assert tomllib.loads(config.read_text())['default_profile'] == 'shell'
        assert profile_command("printf '%s' $$", 'profile-after-delete') == split_pid
        app_shortcut(ord('t'))
        assert profile_command("printf '%s' \"$PWD\"", 'profile-fallback-cwd') == str(Path.home())
        print('Profile create/default/launch, Light/custom themes, live edits, default splits, and deletion without restarting work: passed')
        print('Settings Save/Cancel, live resize with same shell PID, external conflict/replace, corrupt file recovery, failed Save/retry: passed')
        # Keep a shell variable/PID and background work across actual window exit.
        pid_marker = Path(state.name) / 'pid'
        work_marker = Path(state.name) / 'background'
        restored_marker = Path(state.name) / 'restored'
        type_text("export OPTERM_GUI=still-here; printf '%s' $$ > '" + str(pid_marker) + "'; (sleep 0.5; printf ALIVE > '" + str(work_marker) + "') &\n")
        wait_until(pid_marker.exists)
        original_pid = pid_marker.read_text()
        close_window(window)
        assert app.wait(timeout=5) == 0, 'Normal close failed'
        wait_until(lambda: work_marker.exists() and work_marker.read_text() == 'ALIVE')
        app, window = reopen()
        type_text("printf '%s:%s' $$ \"$OPTERM_GUI\" > '" + str(restored_marker) + "'\n")
        wait_until(lambda: restored_marker.exists() and restored_marker.read_text() == original_pid + ':still-here')
        print('Normal window close/reopen: same shell PID, variables, and background work passed')
        work_marker.unlink()
        ready = Path(state.name) / 'ready'
        type_text("(sleep 0.5; printf ALIVE > '" + str(work_marker) + "') & printf READY > '" + str(ready) + "'\n")
        wait_until(ready.exists)
        app.kill(); app.wait(timeout=5)
        wait_until(lambda: work_marker.exists() and work_marker.read_text() == 'ALIVE')
        restored_marker.unlink()
        app, window = reopen()
        type_text("printf '%s:%s' $$ \"$OPTERM_GUI\" > '" + str(restored_marker) + "'\n")
        wait_until(lambda: restored_marker.exists() and restored_marker.read_text() == original_pid + ':still-here')
        print('Desktop SIGKILL/reopen: same shell PID, variables, and background work passed')
        left_pid = command_file("export PANE=left; printf '%s' $$", 'left-pid')
        left_size = command_file('stty size', 'left-size')
        shortcut(ord('e'))
        right_pid = command_file("export PANE=right; printf '%s' $$", 'right-pid')
        shortcut(ord('o'))
        bottom_pid = command_file("export PANE=bottom; printf '%s' $$", 'bottom-pid')
        assert len({left_pid, right_pid, bottom_pid}) == 3, 'Split reused a shell process'
        bottom_size = command_file('stty size', 'bottom-size')
        click(100, 150)
        assert command_file("printf '%s:%s' $$ \"$PANE\"", 'mouse-focus') == left_pid + ':left'
        shortcut(0xff53)  # Right
        assert command_file("printf '%s:%s' $$ \"$PANE\"", 'key-right') == right_pid + ':right'
        shortcut(0xff54)  # Down
        assert command_file("printf '%s:%s' $$ \"$PANE\"", 'key-down') == bottom_pid + ':bottom'
        shortcut(0xff52, alt=True)  # Resize nearest horizontal divider upwards.
        resized = command_file('stty size', 'bottom-resized')
        assert int(resized.split()[0]) > int(bottom_size.split()[0]), (bottom_size, resized)
        XT.XTestFakeMotionEvent(dpy, -1, 500, 200, 0)
        XT.XTestFakeButtonEvent(dpy, 1, 1, 0); X.XFlush(dpy); time.sleep(0.2)
        XT.XTestFakeMotionEvent(dpy, -1, 430, 200, 0); X.XFlush(dpy); time.sleep(0.2)
        XT.XTestFakeButtonEvent(dpy, 1, 0, 0); X.XFlush(dpy); time.sleep(0.3)
        click(100, 150)
        dragged = command_file('stty size', 'left-dragged')
        assert int(dragged.split()[1]) < int(left_size.split()[1]) // 2, (left_size, dragged)
        shortcut(0xff0d)
        maximized = command_file('stty size', 'left-maximized')
        assert int(maximized.split()[1]) > int(dragged.split()[1])
        shortcut(0xff53)
        assert command_file("printf '%s' $$", 'maximized-next') == right_pid
        shortcut(0xff51)
        assert command_file("printf '%s' $$", 'maximized-previous') == left_pid
        shortcut(0xff0d)
        restored = command_file('stty size', 'left-restored')
        assert restored == dragged, (dragged, restored)
        rapid = Path(state.name) / 'rapid-focus'
        type_text("printf '%s' $$ > '" + str(rapid) + "'\n")
        click(700, 120)
        wait_until(lambda: rapid.exists() and rapid.stat().st_size > 0)
        assert rapid.read_text() == left_pid, 'A later click redirected earlier keyboard input'
        # All visible panes must receive updates, including ones without keyboard focus.
        for x, y, name, expected in [(100,150,'Left shell',left_pid), (700,120,'Top right',right_pid), (700,500,'Bottom right',bottom_pid)]:
            click(x,y)
            assert command_file("printf '%s' $$", name + "-focus") == expected, name
            type_text("printf '\\033[2J\\033[H\\033]0;" + name + "\\007" + name + "\\n'; (sleep 0.4; printf 'Background output arrives here.\\n') &\n")
            colors = "printf '\\033[31mred \\033[32mgreen \\033[34mblue\\033[0m\\n\\033[38;5;208m256-color orange\\033[0m\\n\\033[38;2;12;170;240mTruecolor cyan\\033[0m\\n'; printf READY"
            command_file(colors, name + '-rendered')
        time.sleep(0.7)
        print('Nested splits, isolated input, mouse/keyboard focus, drag/keyboard resize, maximize/restore: passed')
        print('Split screenshot:', screenshot(display, 'prototype-splits-linux.png'))
        detached = Path(state.name) / 'pane-detached'
        ready = Path(state.name) / 'pane-ready'
        type_text("(sleep 0.5; printf ALIVE > '" + str(detached) + "') & printf READY > '" + str(ready) + "'\n")
        wait_until(ready.exists)
        shortcut(ord('w'))
        wait_until(lambda: detached.exists() and detached.read_text() == 'ALIVE')
        click(700, 150)
        assert command_file("printf '%s:%s' $$ \"$PANE\"", 'collapsed-focus') == right_pid + ':right'
        print('Closing a split collapses its branch and leaves its shell running: passed')
        # New Window opens fresh work rather than taking over another view.
        before = set(find_windows())
        shortcut(ord('n'))
        second = wait_until(lambda: next(iter(set(find_windows()) - before), None))
        X.XMapWindow(dpy, second); X.XSync(dpy, 0); time.sleep(0.3)
        X.XSetInputFocus(dpy, second, 1, 0); X.XFlush(dpy); time.sleep(0.4)
        new_pid = command_file("export NEW_WINDOW=alive; printf '%s' $$", 'window-pid')
        assert new_pid not in {left_pid, right_pid, bottom_pid}
        close_window(window)
        assert app.wait(timeout=5) == 0
        assert command_file("printf '%s:%s' $$ \"$NEW_WINDOW\"", 'window-survived') == new_pid + ':alive'
        close_window(second)
        wait_until(lambda: not find_windows())
        print('New Window starts a fresh shell and survives closing the first window: passed')
        # Reopen both saved windows, preserving each window's tabs/splits and focus.
        layout_folder = Path(env['OPTERM_STATE_DIR']) / 'layouts'
        documents = sorted(layout_folder.glob('*.json'))
        assert len(documents) == 2, documents
        expected = {json.loads(path.read_text())['id']: json.loads(path.read_text()) for path in documents}
        split_doc = next(doc for doc in expected.values() if len(doc['tabs']) > 1)
        # Give the closed window a distinctive manual name and bounds to check on restore.
        split_doc['tabs'][split_doc['active']]['name'] = 'Restored splits'
        split_doc['size'] = [900, 620]; split_doc['position'] = [30, 40]
        (layout_folder / (split_doc['id'] + '.json')).write_text(json.dumps(split_doc))
        def title_of(w):
            value = C.c_void_p()
            X.XFetchName(dpy, w, C.byref(value))
            title = C.string_at(value).decode() if value else ''
            if value: X.XFree(value)
            return title
        app = subprocess.Popen([str(binary)], cwd=ROOT, env=env)
        wait_until(lambda: len(find_windows()) == 2)
        time.sleep(0.6)
        restored_windows = find_windows()
        seen = set()
        for current in restored_windows:
            X.XMapWindow(dpy, current); X.XSetInputFocus(dpy, current, 1, 0); X.XFlush(dpy); time.sleep(0.35)
            pid = command_file("printf '%s' $$", 'restored-window-' + str(current))
            seen.add(pid)
            if pid == right_pid:
                restored_path = layout_folder / (split_doc['id'] + '.json')
                wait_until(lambda: json.loads(restored_path.read_text())['size'] == [900, 620])
                assert json.loads(restored_path.read_text())['position'] == [30, 40]
                assert json.loads(restored_path.read_text())['tabs'][split_doc['active']]['layout'] == split_doc['tabs'][split_doc['active']]['layout']
                assert 'Restored splits' in title_of(current), title_of(current)
                assert command_file("printf '%s' \"$PANE\"", 'restored-pane-state') == 'right'
                shortcut(0xff51)
                assert command_file("printf '%s:%s' $$ \"$PANE\"", 'restored-left') == left_pid + ':left'
                shortcut(0xff53)
                assert command_file("printf '%s' $$", 'restored-right') == right_pid
                # Save temporary maximize state while retaining the underlying split.
                shortcut(0xff0d)
                wait_until(lambda: json.loads((layout_folder / (split_doc['id'] + '.json')).read_text())['tabs'][split_doc['active']]['maximized'])
            elif pid == new_pid:
                assert command_file("printf '%s' \"$NEW_WINDOW\"", 'restored-new-window') == 'alive'
            else: raise AssertionError(('Restored a replacement shell', pid, seen))
        assert seen == {right_pid, new_pid}, seen
        print('Two saved windows restore tab names, nested splits, focused panes, and original shell state: passed')
        print('Restored layout screenshot:', screenshot(display, 'prototype-layouts-linux.png'))
        for current in restored_windows: close_window(current)
        wait_until(lambda: not find_windows())
        app.wait(timeout=5)
        before_restart = {path.name: json.loads(path.read_text())['tabs'] for path in documents}
        # Restarting the service must leave placeholders instead of replaying old commands.
        subprocess.run([str(binary), '--stop-service'], env=env, check=True, stdout=subprocess.DEVNULL)
        app = subprocess.Popen([str(binary)], cwd=ROOT, env=env)
        wait_until(lambda: len(find_windows()) == 2)
        time.sleep(1.3)
        for path in documents:
            doc = json.loads(path.read_text())
            assert doc['tabs'], 'Unavailable work was silently discarded'
            assert doc['tabs'] == before_restart[path.name], 'Unavailable references/layout were replaced'
        with socket.socket(socket.AF_UNIX) as probe:
            probe.connect(str(Path(env['OPTERM_STATE_DIR']) / 'service.sock'))
            def call(request):
                data = json.dumps(request).encode(); probe.sendall(struct.pack('<I', len(data)) + data)
                def receive(count):
                    data = b''
                    while len(data) < count:
                        chunk = probe.recv(count - len(data)); assert chunk; data += chunk
                    return data
                length = struct.unpack('<I', receive(4))[0]
                return json.loads(receive(length))
            call({'Hello': 1})
            assert call('List') == {'List': []}, 'Restoration created replacement shells after service restart'
        print('Service restart restores unavailable panes without replaying commands: passed')

        controls()


    except Exception:
        if dpy: print('Failure screenshot:', screenshot(display, 'gui-failure.png'))
        raise
    finally:
        if dpy:
            for remaining in find_windows(): close_window(remaining)
        if app:
            app.terminate()
            try:
                app.wait(timeout=3)
            except subprocess.TimeoutExpired:
                app.kill(); app.wait()
        if env:
            subprocess.run([str(binary), '--stop-service'], cwd=ROOT, env=env, timeout=5, check=True, stdout=subprocess.DEVNULL)
        if env and Path(env['OPTERM_RECOVERY_DIR']).exists():
            Path(env['OPTERM_RECOVERY_DIR']).chmod(0o700)
        state.cleanup()
        if dpy:
            X.XCloseDisplay(dpy)
        server.terminate(); server.wait(timeout=3)


if __name__ == '__main__':
    main()
