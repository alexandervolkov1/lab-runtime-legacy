"""Windows-only observer of the existing v1 native smoke; no production hooks.

Run after building both binaries: python scripts/support/v1-parent-pump-smoke.py
--profile dev|release. Uses a fresh virtual deployment/workspace under target.
One bounded test proxy observes actual Application requests across reconnect and
holds the sole retune before forwarding while capturing the visible OS window.
This is neither a framebuffer readback nor a mouse/keyboard Confirm test.
"""

import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import socket
import struct
import subprocess
import sys
import threading
import time
import zlib


ROOT = Path(__file__).resolve().parents[2]
BUFFER_LIMIT = 1_048_576
REQUEST_LIMIT = 256
CONNECTION_LIMIT = 4


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(pid, path):
    """Capture this process's visible window client area using Win32 GDI."""
    user, gdi = c.WinDLL("user32", use_last_error=True), c.WinDLL("gdi32", use_last_error=True)
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.EnumWindows.argtypes = [callback_type, w.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
    user.IsWindowVisible.argtypes = [w.HWND]
    user.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
    user.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    user.GetDC.argtypes, user.GetDC.restype = [w.HWND], w.HDC
    user.ReleaseDC.argtypes = [w.HWND, w.HDC]
    gdi.CreateCompatibleDC.argtypes, gdi.CreateCompatibleDC.restype = [w.HDC], w.HDC
    gdi.CreateCompatibleBitmap.argtypes, gdi.CreateCompatibleBitmap.restype = [w.HDC, c.c_int, c.c_int], w.HBITMAP
    gdi.SelectObject.argtypes, gdi.SelectObject.restype = [w.HDC, w.HGDIOBJ], w.HGDIOBJ
    gdi.BitBlt.argtypes = [w.HDC, c.c_int, c.c_int, c.c_int, c.c_int, w.HDC, c.c_int, c.c_int, w.DWORD]
    gdi.GetDIBits.argtypes = [w.HDC, w.HBITMAP, w.UINT, w.UINT, c.c_void_p, c.c_void_p, w.UINT]
    gdi.DeleteObject.argtypes, gdi.DeleteDC.argtypes = [w.HGDIOBJ], [w.HDC]
    found = []

    @callback_type
    def visit(hwnd, _):
        owner = w.DWORD()
        user.GetWindowThreadProcessId(hwnd, c.byref(owner))
        title = c.create_unicode_buffer(256)
        user.GetWindowTextW(hwnd, title, len(title))
        if owner.value == pid and user.IsWindowVisible(hwnd) and title.value == "lab-runtime Workbench":
            found.append(hwnd)
        return True

    user.EnumWindows(visit, 0)
    assert len(found) == 1, f"expected one visible Workbench window, got {len(found)}"
    rect = w.RECT()
    assert user.GetClientRect(found[0], c.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    assert 0 < width <= 4096 and 0 < height <= 4096
    screen = user.GetDC(found[0])
    dc = gdi.CreateCompatibleDC(screen)
    bitmap = gdi.CreateCompatibleBitmap(screen, width, height)
    previous = gdi.SelectObject(dc, bitmap)
    try:
        assert screen and dc and bitmap and previous
        assert gdi.BitBlt(dc, 0, 0, width, height, screen, 0, 0, 0x00CC0020)
        gdi.SelectObject(dc, previous)
        previous = None
        header = struct.pack("<IiiHHIIiiII", 40, width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0)
        info = c.create_string_buffer(header + bytes(4))
        pixels = c.create_string_buffer(width * height * 4)
        assert gdi.GetDIBits(dc, bitmap, 0, height, pixels, info, 0) == height
        bgra = pixels.raw
        rgb = bytearray(width * height * 3)
        rgb[0::3], rgb[1::3], rgb[2::3] = bgra[2::4], bgra[1::4], bgra[0::4]
        stride = width * 3
        scanlines = b"".join(b"\0" + rgb[row * stride:(row + 1) * stride] for row in range(height))

        def chunk(kind, payload):
            return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))

        path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                         + chunk(b"IDAT", zlib.compress(scanlines)) + chunk(b"IEND", b""))
        return {"method": "visible window client GDI capture, not GL readback", "width": width, "height": height,
                "sha256": sha(path), "path": str(path.relative_to(ROOT))}
    finally:
        if previous:
            gdi.SelectObject(dc, previous)
        gdi.DeleteObject(bitmap)
        gdi.DeleteDC(dc)
        user.ReleaseDC(found[0], screen)


class Proxy:
    """Single test I/O thread, at most four sessions, finite buffers/request log."""
    def __init__(self, port, hold_first_runtime_close=False):
        self.port = port
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(CONNECTION_LIMIT)
        self.listener.setblocking(False)
        self.stop, self.retune, self.release = threading.Event(), threading.Event(), threading.Event()
        self.samples, self.signal_events = threading.Condition(), 0
        self.session_ends, self.closures = threading.Condition(), []
        self.requests, self.connections, self.error = [], 0, None
        self.sessions = []
        self.hold_first_runtime_close = hold_first_runtime_close
        self.thread = threading.Thread(target=self.run, name="native-smoke-observer")
        self.thread.start()

    def run(self):
        peers, clients, partners, outgoing, incoming, held = set(), set(), {}, {}, {}, set()
        session_ids, retained = {}, set()

        def close_peer(peer):
            peers.discard(peer)
            clients.discard(peer)
            held.discard(peer)
            incoming.pop(peer, None)
            outgoing.pop(peer, None)
            peer.close()

        def release_retained():
            for peer in retained:
                self.sessions[session_ids[peer]]["runtime_close_released_ns"] = time.monotonic_ns()
                close_peer(peer)
            retained.clear()

        def end_client(child, mode):
            # A Windows TCP close with unread event bytes may be a reset, not EOF.
            # All complete Runtime-bound frames are recorded before forwarding;
            # discarded partial frames cannot become admitted Runtime requests.
            index = session_ids[child]
            self.sessions[index]["gui_close"] = mode
            self.sessions[index]["gui_close_ns"] = time.monotonic_ns()
            runtime = partners[child]
            close_peer(child)
            if self.hold_first_runtime_close and index == 0:
                # Controlled observer experiment only: keep the old Runtime
                # attachment until the next hello receives scope_in_use.
                peers.discard(runtime)
                retained.add(runtime)
                self.sessions[index]["runtime_close_held"] = True
            else:
                close_peer(runtime)
            with self.session_ends:
                self.closures.append(mode)
                self.session_ends.notify_all()

        try:
            while not self.stop.is_set():
                if self.release.is_set():
                    held.clear()
                readable, writable, _ = select.select([self.listener, *peers],
                    [p for p in peers if outgoing[p] and p not in held], [], 0.05)
                for peer in readable:
                    if peer is self.listener:
                        child, _ = self.listener.accept()
                        self.connections += 1
                        assert self.connections <= CONNECTION_LIMIT
                        self.sessions.append({"session": self.connections, "opened_ns": time.monotonic_ns(),
                                              "request_indices": [], "hello_responses": []})
                        runtime = socket.create_connection(("127.0.0.1", self.port), timeout=2)
                        for p in (child, runtime):
                            session_ids[p] = self.connections - 1
                            p.setblocking(False)
                            peers.add(p)
                            outgoing[p] = bytearray()
                        partners[child], partners[runtime] = runtime, child
                        clients.add(child)
                        incoming[child], incoming[runtime] = bytearray(), bytearray()
                        continue
                    if peer not in peers:
                        continue
                    try:
                        data = peer.recv(16_384)
                    except ConnectionResetError:
                        assert peer in clients, "unexpected Runtime-side reset"
                        end_client(peer, "gui_reset_on_receive")
                        continue
                    if not data:
                        assert peer in clients, "Runtime closed before GUI EOF"
                        end_client(peer, "gui_eof")
                        continue
                    target = partners[peer]
                    outgoing[target].extend(data)
                    assert len(outgoing[target]) <= BUFFER_LIMIT
                    if peer in incoming:
                        incoming[peer].extend(data)
                        assert len(incoming[peer]) <= BUFFER_LIMIT
                        while b"\n" in incoming[peer]:
                            frame, _, rest = incoming[peer].partition(b"\n")
                            incoming[peer] = bytearray(rest)
                            message = json.loads(frame)
                            if peer in clients:
                                assert len(self.requests) < REQUEST_LIMIT
                                self.requests.append(message)
                                self.sessions[session_ids[peer]]["request_indices"].append(len(self.requests) - 1)
                                if message["op"] == "reference_retune":
                                    held.add(target)
                                    self.retune.set()
                            elif message.get("msg_id") == "1":
                                responses = self.sessions[session_ids[peer]]["hello_responses"]
                                assert not responses, "duplicate hello response"
                                responses.append({"observed_ns": time.monotonic_ns(), "message": message})
                                if message.get("code") == "scope_in_use":
                                    release_retained()
                            elif message.get("type") == "event" and message.get("kind") == "signal":
                                with self.samples:
                                    self.signal_events += 1
                                    self.samples.notify_all()
                for peer in writable:
                    if peer in peers and peer not in held:
                        try:
                            count = peer.send(outgoing[peer])
                        except ConnectionResetError:
                            assert peer in clients, "unexpected Runtime-side reset"
                            end_client(peer, "gui_reset_on_send")
                            continue
                        del outgoing[peer][:count]
        except Exception as error:
            self.error = repr(error)
            print(json.dumps({"observer_error": self.error, "connections": self.connections,
                              "closures": self.closures}), flush=True)
            self.retune.set()
        finally:
            release_retained()
            for peer in peers:
                peer.close()
            self.listener.close()

    def close(self):
        self.stop.set()
        self.release.set()
        self.thread.join(timeout=3)
        assert not self.thread.is_alive(), "observer stop unconfirmed"
        assert self.error is None, self.error


def check_reset():
    """Deterministically reproduce an abortive GUI-side TCP close, then reconnect."""
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(2)
    listener.settimeout(3)
    errors = []

    def fake_runtime():
        try:
            for _ in range(2):
                with listener.accept()[0] as peer:
                    peer.settimeout(3)
                    frame = bytearray()
                    while b"\n" not in frame:
                        data = peer.recv(1024)
                        assert data and len(frame) < 4096
                        frame.extend(data)
                    peer.sendall(b'{"type":"event","kind":"signal"}\n')
                    assert peer.recv(1) == b""
        except Exception as error:
            errors.append(repr(error))
        finally:
            listener.close()

    server = threading.Thread(target=fake_runtime, name="smoke-reset-fixture")
    server.start()
    proxy = Proxy(listener.getsockname()[1])
    try:
        for index in range(2):
            with socket.create_connection(proxy.listener.getsockname(), timeout=3) as child:
                child.sendall(b'{"v":1,"msg_id":"1","op":"hello","args":{}}\n')
                if index == 0:
                    assert child.recv(1)  # Remaining event bytes deliberately unread.
                    child.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("hh", 1, 0))
                else:
                    frame = bytearray()
                    while b"\n" not in frame:
                        data = child.recv(1024)
                        assert data and len(frame) < 4096
                        frame.extend(data)
                    child.shutdown(socket.SHUT_RDWR)
            with proxy.session_ends:
                assert proxy.session_ends.wait_for(lambda: len(proxy.closures) == index + 1, timeout=3)
        assert proxy.closures[0].startswith("gui_reset_") and proxy.closures[1] == "gui_eof"
        assert proxy.connections == 2 and len(proxy.requests) == 2
        server.join(timeout=3)
        assert not server.is_alive() and not errors, errors
        print(json.dumps({"reset_regression": "pass", "closures": proxy.closures, "requests": 2}))
    finally:
        proxy.close()
        server.join(timeout=3)


def main():
    args = argparse.ArgumentParser()
    args.add_argument("--profile", choices=("dev", "release"), required=True)
    args.add_argument("--binary-directory", type=Path)
    args.add_argument("--binary-source-head")
    args.add_argument("--hold-first-runtime-close", action="store_true",
                      help="negative observer experiment; exact-two-session assertion stays strict")
    options = args.parse_args()
    profile = options.profile
    assert (options.binary_directory is None) == (options.binary_source_head is None)
    observer_source_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    binary_source_head = options.binary_source_head or observer_source_head
    assert os.name == "nt", "requires Windows desktop and MSVC binaries"
    directory = ROOT / "target" / ("b3b1-followup-native-" + profile + "-" + str(time.time_ns()))
    directory.mkdir()
    config = (ROOT / "examples/runtime.virtual.toml").read_text(encoding="utf-8")
    config = config.replace("port = 7420", "port = 0").replace('path = "history.sqlite"',
        'path = "' + (directory / "history.sqlite").as_posix() + '"')
    config_path = directory / "runtime.toml"
    config_path.write_text(config, encoding="utf-8")
    binaries = options.binary_directory or ROOT / "target" / ("debug" if profile == "dev" else "release")
    env = dict(os.environ)
    env["LAB_RUNTIME_LOG_DIRECTORY"] = str(directory / "diagnostics")
    env["LAB_WORKBENCH_GUI_SMOKE_RESULT"] = str(directory / "smoke.json")
    runtime = child = proxy = None
    with (directory / "runtime.log").open("wb") as runtime_log, (directory / "workbench.log").open("wb") as child_log:
        try:
            runtime = subprocess.Popen([str(binaries / "lab-runtime.exe"), "--serve", "--config", str(config_path)],
                stdout=runtime_log, stderr=runtime_log, env=env, creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
            deadline = time.monotonic() + 15
            ready = None
            while ready is None and time.monotonic() < deadline:
                assert runtime.poll() is None, "Runtime exited before readiness"
                for line in (directory / "runtime.log").read_text(encoding="utf-8", errors="replace").splitlines():
                    if line.startswith('{"boot_id"'):
                        ready = json.loads(line)
                threading.Event().wait(0.02)  # Poll observed readiness; no guessed startup delay.
            assert ready is not None, "Runtime readiness deadline"
            proxy = Proxy(ready["port"], options.hold_first_runtime_close)
            child = subprocess.Popen([str(binaries / "lab-workbench.exe"), "--connect",
                "127.0.0.1:" + str(proxy.listener.getsockname()[1]), "--workspace", str(directory / "workspace")],
                stdout=child_log, stderr=child_log, env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            assert proxy.retune.wait(15), "existing smoke did not submit retune"
            assert proxy.error is None, proxy.error
            with proxy.samples:
                assert proxy.samples.wait_for(lambda: proxy.signal_events >= 5, timeout=5), "signal event deadline"
            screenshot = capture(child.pid, directory / "window.png")
            proxy.release.set()
            exit_code = child.wait(timeout=35)
            result = json.loads((directory / "smoke.json").read_text(encoding="utf-8"))
            assert exit_code == 0 and result["status"] == "pass", result
            with proxy.session_ends:
                assert proxy.session_ends.wait_for(lambda: len(proxy.closures) == 2, timeout=3), "GUI transport-close deadline"
            proxy.close()
            mutations = [r for r in proxy.requests if "request_id" in r]
            statuses = [r for r in proxy.requests if r["op"] == "operation_status"]
            assert len(mutations) == 1 and mutations[0]["op"] == "reference_retune", mutations
            assert not statuses, statuses
            assert proxy.connections == 2, proxy.connections
            assert result["presentation_plots"] > 0 and result["live_signal_points"] > 0
            assert all(result[key] for key in ("confirmation_observed", "mutation_accepted_observed",
                "mutation_completed_observed", "authoritative_refresh_observed", "stale_controls_disabled",
                "controls_reenabled_after_fresh", "minimize_restore_observed"))
            record = {"profile": profile, "rust_source_head": binary_source_head,
                "observer_source_head": observer_source_head, "gui_exit_code": exit_code,
                "native_smoke": result, "connections": proxy.connections, "requests": proxy.requests,
                "client_transport_closures": proxy.closures,
                "sessions": proxy.sessions,
                "binary_source_head": binary_source_head,
                "held_first_runtime_close": options.hold_first_runtime_close,
                "mutation_submissions": len(mutations), "status_submissions": len(statuses),
                "repeat_mutation_submissions": 0, "window_capture": screenshot,
                "observed_signal_events": proxy.signal_events,
                "binaries_sha256": {name: sha(binaries / name) for name in ("lab-runtime.exe", "lab-workbench.exe")},
                "scope": "existing automated Confirm semantics; visible window capture; no mouse Confirm or GL readback"}
            (directory / "result.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
            print(json.dumps({"status": "pass", "result": str(directory / "result.json")}))
        finally:
            if proxy is not None:
                (directory / "observer.json").write_text(json.dumps({"error": proxy.error,
                    "connections": proxy.connections, "client_transport_closures": proxy.closures,
                    "requests": proxy.requests, "sessions": proxy.sessions,
                    "binary_source_head": binary_source_head, "observer_source_head": observer_source_head,
                    "held_first_runtime_close": options.hold_first_runtime_close}, indent=2) + "\n", encoding="utf-8")
            if proxy is not None and proxy.thread.is_alive():
                proxy.close()
            if child is not None and child.poll() is None:
                child.kill()  # Test containment on failure, never a healthy shutdown receipt.
                child.wait(timeout=3)
            if runtime is not None and runtime.poll() is None:
                try:
                    runtime.send_signal(signal.CTRL_BREAK_EVENT)
                    runtime.wait(timeout=10)
                finally:
                    if runtime.poll() is None:
                        runtime.kill()  # Virtual fixture containment only.
                        runtime.wait(timeout=3)


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test-reset"]:
        check_reset()
    else:
        main()
