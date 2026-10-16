"""Windows-only fixture driver: stock screenshots and PID-scoped native input.

Never injects egui events or calls a graphics API. All OS actions target the
original HWND reported by this one Job child, with a PID check before each call.
"""
import ctypes as c
from ctypes import wintypes as w
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
PROFILE, RUN = sys.argv[1:3]
assert PROFILE in ("dev", "release") and RUN.replace("-", "").isalnum()
OUT = ROOT / "target/eframe-1" / RUN
OUT.mkdir(parents=True, exist_ok=False)
SOURCE = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
assert not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all", "--", "apps", "crates", "scripts", "Cargo.toml", "Cargo.lock"], cwd=ROOT).strip(), "Commit executable inputs first"
MANIFEST = "scripts/support/eframe-host/Cargo.toml"
BINARY = ROOT / "scripts/support/eframe-host/target" / ("debug" if PROFILE == "dev" else "release") / "lab-eframe-host.exe"


def command(name, args):
    start = time.monotonic()
    print("START", name, flush=True)
    with (OUT / (name + ".log")).open("wb") as stream:
        result = subprocess.run(args, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
    receipt = dict(source=SOURCE, command=args, exit_code=result.returncode,
                   elapsed_seconds=time.monotonic()-start,
                   utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                   log_sha256=hashlib.sha256((OUT / (name + ".log")).read_bytes()).hexdigest())
    (OUT / (name + ".json")).write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    assert result.returncode == 0, receipt


for name, args in [
    ("fmt", ["cargo", "fmt", "--manifest-path", MANIFEST, "--", "--check"]),
    ("clippy", ["cargo", "clippy", "--manifest-path", MANIFEST, "--offline", "--locked", "--profile", PROFILE, "--", "-D", "warnings"]),
    ("build", ["cargo", "build", "--manifest-path", MANIFEST, "--offline", "--locked", "--profile", PROFILE]),
]:
    command(name, args)
(OUT / "binary.json").write_text(json.dumps(dict(source=SOURCE, profile=PROFILE,
    binary=str(BINARY), sha256=hashlib.sha256(BINARY.read_bytes()).hexdigest()), indent=2))

user = c.WinDLL("user32", use_last_error=True)
user.SetProcessDpiAwarenessContext.argtypes = [c.c_void_p]
user.SetProcessDpiAwarenessContext.restype = w.BOOL
assert user.SetProcessDpiAwarenessContext(c.c_void_p(-4)), c.get_last_error()
user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
user.GetWindowThreadProcessId.restype = w.DWORD
user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
user.PostMessageW.restype = w.BOOL
user.SetWindowPos.argtypes = [w.HWND, w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.UINT]
user.SetWindowPos.restype = w.BOOL
user.ShowWindow.argtypes = [w.HWND, c.c_int]
user.ShowWindow.restype = w.BOOL
user.IsIconic.argtypes = [w.HWND]
user.IsIconic.restype = w.BOOL
user.SetForegroundWindow.argtypes = [w.HWND]
user.SetForegroundWindow.restype = w.BOOL
user.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
user.GetWindowRect.restype = w.BOOL
user.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
user.GetClientRect.restype = w.BOOL
user.GetDpiForWindow.argtypes = [w.HWND]
user.GetDpiForWindow.restype = w.UINT
user.GetForegroundWindow.restype = w.HWND
user.SetFocus.argtypes = [w.HWND]
user.SetFocus.restype = w.HWND
user.AttachThreadInput.argtypes = [w.DWORD, w.DWORD, w.BOOL]
user.AttachThreadInput.restype = w.BOOL
user.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
user.ClientToScreen.restype = w.BOOL
user.SetCursorPos.argtypes = [c.c_int, c.c_int]
user.SetCursorPos.restype = w.BOOL
kernel = c.WinDLL("kernel32", use_last_error=True)
kernel.GetCurrentThreadId.restype = w.DWORD


class Keyboard(c.Structure):
    _fields_ = [("virtual", w.WORD), ("scan", w.WORD), ("flags", w.DWORD),
                ("time", w.DWORD), ("extra", c.c_size_t)]


class Mouse(c.Structure):
    _fields_ = [("x", w.LONG), ("y", w.LONG), ("data", w.DWORD),
                ("flags", w.DWORD), ("time", w.DWORD), ("extra", c.c_size_t)]


class InputPayload(c.Union):
    _fields_ = [("keyboard", Keyboard), ("mouse", Mouse)]


class Input(c.Structure):
    _fields_ = [("kind", w.DWORD), ("payload", InputPayload)]


assert c.sizeof(Input) == 40, "Windows x64 INPUT ABI"
user.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
user.SendInput.restype = w.UINT


def wait_json(path, process, seconds=25):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if path.exists():
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, PermissionError):
                pass
        assert process.poll() is None, f"child/controller exited waiting for {path}"
        time.sleep(.03)
    raise TimeoutError(str(path))


for renderer in ("wgpu", "glow"):
    for mode in ("normal", "crash", "stall"):
        folder = OUT / (renderer + "-" + mode)
        actions = []
        log_path = OUT / (renderer + "-" + mode + ".log")
        with log_path.open("wb") as stream:
            process = subprocess.Popen([str(BINARY), "--launch", renderer, str(folder)], cwd=ROOT,
                                       stdout=stream, stderr=subprocess.STDOUT)
            try:
                ready = wait_json(folder / "ready.json", process)
                hwnd, pid = ready["hwnd"], ready["pid"]

                def owner():
                    actual = w.DWORD()
                    assert user.GetWindowThreadProcessId(hwnd, c.byref(actual))
                    assert actual.value == pid, "original window ownership changed"

                def post(message, first=0, second=0):
                    owner()
                    assert user.PostMessageW(hwnd, message, first, second), c.get_last_error()
                    actions.append(dict(message=message, wparam=first, lparam=second))

                def click(point):
                    owner()
                    assert user.GetForegroundWindow() == hwnd
                    location = w.POINT(*map(round, point))
                    assert user.ClientToScreen(hwnd, c.byref(location))
                    assert user.SetCursorPos(location.x, location.y)
                    time.sleep(.08)
                    for flags in (2, 4):
                        owner()
                        assert user.GetForegroundWindow() == hwnd
                        event = Input()
                        event.kind = 0
                        event.payload.mouse.flags = flags
                        assert user.SendInput(1, c.byref(event), c.sizeof(Input)) == 1
                        time.sleep(.08)
                    actions.append(dict(mouse_click_client_pixels=point, original_foreground_verified=True))
                    time.sleep(.12)

                frame = wait_json(folder / "frame-0.json", process)
                if mode != "normal":
                    (folder / "stage").write_text("6" if mode == "crash" else "7")
                    wait_json(folder / (mode + "-entered.json"), process)
                    if mode == "stall":
                        (folder / "contain").write_text("explicit test containment after observed owner stall")
                else:
                    owner()
                    # Temporarily join input queues to request real OS focus;
                    # detach in finally. Never fake egui/WM_SETFOCUS events.
                    user.GetQueueStatus(w.UINT(0x4FF))
                    current = kernel.GetCurrentThreadId()
                    foreground = user.GetForegroundWindow()
                    threads = {user.GetWindowThreadProcessId(hwnd, None)}
                    if foreground:
                        threads.add(user.GetWindowThreadProcessId(foreground, None))
                    attached = []
                    try:
                        for thread in threads - {0, current}:
                            assert user.AttachThreadInput(current, thread, True), c.get_last_error()
                            attached.append(thread)
                        focused = bool(user.SetForegroundWindow(hwnd))
                        user.SetFocus(hwnd)
                    finally:
                        for thread in reversed(attached):
                            assert user.AttachThreadInput(current, thread, False), c.get_last_error()
                    time.sleep(.2)
                    assert user.GetForegroundWindow() == hwnd
                    actions.append(dict(foreground_requested=True, accepted=focused,
                                        actual_window_dpi=user.GetDpiForWindow(hwnd)))
                    click(frame["button_pixels"])
                    click(frame["editor_pixels"])
                    # winit pairs CHAR with a preceding key event; bare WM_CHAR
                    # is ignored. Let Windows produce the full Unicode sequence.
                    # Send only while our original child owns foreground focus.
                    for char in "TestЖ":
                        owner()
                        assert user.GetForegroundWindow() == hwnd
                        pair = (Input * 2)()
                        for index in range(2):
                            pair[index].kind = 1
                            pair[index].payload.keyboard = Keyboard(0, ord(char), 4 | (2 if index else 0), 0, 0)
                        assert user.SendInput(2, pair, c.sizeof(Input)) == 2
                        actions.append(dict(unicode_send_input=ord(char), original_foreground_verified=True))
                    time.sleep(.2)
                    post(0x0100, 0x25, (0x4B << 16) | 1)  # left arrow
                    post(0x0101, 0x25, (0x4B << 16) | (3 << 30) | 1)
                    for stage in range(1, 6):
                        if stage in (3, 4, 5):
                            width, height = {3: (800, 600), 4: (1440, 900), 5: (1100, 720)}[stage]
                            owner()
                            outer, client = w.RECT(), w.RECT()
                            assert user.GetWindowRect(hwnd, c.byref(outer))
                            assert user.GetClientRect(hwnd, c.byref(client))
                            assert user.SetWindowPos(hwnd, None, 0, 0,
                                width + outer.right - outer.left - client.right,
                                height + outer.bottom - outer.top - client.bottom, 0x0016)
                            actions.append(dict(resize_client_pixels=[width, height]))
                            time.sleep(.2)
                        if stage == 5:
                            owner()
                            user.ShowWindow(hwnd, 6)  # SW_MINIMIZE
                            time.sleep(.3)
                            assert user.IsIconic(hwnd)
                            actions.append(dict(minimized=True))
                            owner()
                            user.ShowWindow(hwnd, 9)  # SW_RESTORE
                            time.sleep(.3)
                            assert not user.IsIconic(hwnd)
                            actions.append(dict(restored=True))
                        (folder / "stage").write_text(str(stage))
                        frame = wait_json(folder / f"frame-{stage}.json", process)
                        if stage in (3, 4, 5):
                            assert [frame["width"], frame["height"]] == [width, height], frame
                    assert frame["clicked"] == 1, frame
                    assert frame["pointer_moves"] > 0 and frame["pointer_buttons"] >= 4, frame
                    assert frame["texts"] > 0 and "TestЖ" in frame["local_text"], frame
                    assert frame["keys"] > 0, frame
                    assert frame["zoom_factor"] == 1.0, frame
                    assert json.loads((folder / "frame-4.json").read_text())["zoom_factor"] == 1.5
                    post(0x0010)  # WM_CLOSE -> stock winit/eframe CloseRequested
                assert process.wait(timeout=15) == 0
                record = json.loads((folder / "process.json").read_text())
                assert record["passed"] and record["original_handles_retired"], record
                if mode == "normal":
                    assert json.loads((folder / "on-exit.json").read_text())["close_requested"]
                print("PASS", PROFILE, renderer, mode, record, flush=True)
            except BaseException as error:
                actions.append(dict(failure=repr(error)))
                if folder.exists() and process.poll() is None:
                    (folder / "contain").write_text("test driver failure; preserve original receipts")
                    process.wait(timeout=100)
                raise
            finally:
                (OUT / (renderer + "-" + mode + "-actions.json")).write_text(json.dumps(actions, indent=2), encoding="utf-8")
