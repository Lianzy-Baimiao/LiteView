"""Run a released EXE on a private Windows desktop without switching to it.

Developer-side validation, not an end-user diagnostic. Requires Pillow only for
validating the app-owned framebuffer capture. No installer, registry changes,
OS input injection, or access to existing LiteView settings.
"""

import argparse
import ctypes as C
from ctypes import wintypes as W
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import uuid


class STARTUPINFO(C.Structure):
    _fields_ = [
        ("cb", W.DWORD), ("lpReserved", W.LPWSTR), ("lpDesktop", W.LPWSTR),
        ("lpTitle", W.LPWSTR), ("dwX", W.DWORD), ("dwY", W.DWORD),
        ("dwXSize", W.DWORD), ("dwYSize", W.DWORD),
        ("dwXCountChars", W.DWORD), ("dwYCountChars", W.DWORD),
        ("dwFillAttribute", W.DWORD), ("dwFlags", W.DWORD),
        ("wShowWindow", W.WORD), ("cbReserved2", W.WORD),
        ("lpReserved2", C.POINTER(C.c_ubyte)), ("hStdInput", W.HANDLE),
        ("hStdOutput", W.HANDLE), ("hStdError", W.HANDLE),
    ]


class PROCESS_INFORMATION(C.Structure):
    _fields_ = [("hProcess", W.HANDLE), ("hThread", W.HANDLE),
                ("dwProcessId", W.DWORD), ("dwThreadId", W.DWORD)]


def windows_api():
    if os.name != "nt" or C.sizeof(C.c_void_p) != 8:
        raise RuntimeError("Requires 64-bit Windows Python")
    kernel = C.WinDLL("kernel32", use_last_error=True)
    user = C.WinDLL("user32", use_last_error=True)
    signatures = [
        (user.CreateDesktopW, [W.LPCWSTR, W.LPCWSTR, C.c_void_p, W.DWORD, W.DWORD, C.c_void_p], W.HANDLE),
        (user.OpenInputDesktop, [W.DWORD, W.BOOL, W.DWORD], W.HANDLE),
        (user.CloseDesktop, [W.HANDLE], W.BOOL),
        (user.GetUserObjectInformationW, [W.HANDLE, C.c_int, C.c_void_p, W.DWORD, C.POINTER(W.DWORD)], W.BOOL),
        (user.EnumDesktopWindows, [W.HANDLE, C.c_void_p, W.LPARAM], W.BOOL),
        (user.GetWindowThreadProcessId, [W.HWND, C.POINTER(W.DWORD)], W.DWORD),
        (kernel.CreateProcessW, [W.LPCWSTR, W.LPWSTR, C.c_void_p, C.c_void_p, W.BOOL,
                                W.DWORD, C.c_void_p, W.LPCWSTR, C.POINTER(STARTUPINFO),
                                C.POINTER(PROCESS_INFORMATION)], W.BOOL),
        (kernel.WaitForSingleObject, [W.HANDLE, W.DWORD], W.DWORD),
        (kernel.GetExitCodeProcess, [W.HANDLE, C.POINTER(W.DWORD)], W.BOOL),
        (kernel.TerminateProcess, [W.HANDLE, W.UINT], W.BOOL),
        (kernel.CloseHandle, [W.HANDLE], W.BOOL),
        (kernel.SetErrorMode, [W.UINT], W.UINT),
        (kernel.K32EnumProcessModulesEx, [W.HANDLE, C.POINTER(W.HMODULE), W.DWORD, C.POINTER(W.DWORD), W.DWORD], W.BOOL),
        (kernel.K32GetModuleFileNameExW, [W.HANDLE, W.HMODULE, W.LPWSTR, W.DWORD], W.DWORD),
    ]
    for fn, args, result in signatures:
        fn.argtypes, fn.restype = args, result
    return kernel, user


def checked(value):
    if not value:
        raise C.WinError(C.get_last_error())
    return value


def desktop_name(user, handle):
    name, needed = C.create_unicode_buffer(512), W.DWORD()
    checked(user.GetUserObjectInformationW(handle, 2, name, C.sizeof(name), C.byref(needed)))
    return name.value


def input_desktop_name(user):
    handle = checked(user.OpenInputDesktop(0, False, 1))
    try:
        return desktop_name(user, handle)
    finally:
        user.CloseDesktop(handle)


def modules(kernel, process):
    handles, needed = (W.HMODULE * 2048)(), W.DWORD()
    if not kernel.K32EnumProcessModulesEx(process, handles, C.sizeof(handles), C.byref(needed), 3):
        return []  # Loader may not be ready yet or process may have just exited.
    paths = []
    for handle in handles[:min(2048, needed.value // C.sizeof(W.HMODULE))]:
        name = C.create_unicode_buffer(32768)
        if kernel.K32GetModuleFileNameExW(process, handle, name, len(name)):
            paths.append(name.value)
    return paths


def run_private(args, cwd, env, timeout):
    kernel, user = windows_api()
    name = "LiteViewProbe-" + uuid.uuid4().hex
    before = input_desktop_name(user)
    desktop = checked(user.CreateDesktopW(name, None, None, 0, 0xC7, None))
    process = PROCESS_INFORMATION()
    observed, own_windows = set(), set()
    previous_error_mode = kernel.SetErrorMode(0x0001 | 0x0002 | 0x8000)
    start = time.monotonic()
    timed_out = False
    try:
        startup = STARTUPINFO()
        startup.cb = C.sizeof(startup)
        startup.lpDesktop = name
        startup.dwFlags, startup.wShowWindow = 1, 0
        block = C.create_unicode_buffer("\0".join(f"{k}={v}" for k, v in sorted(env.items())) + "\0\0")
        command = C.create_unicode_buffer(subprocess.list2cmdline([str(arg) for arg in args]))
        checked(kernel.CreateProcessW(str(args[0]), command, None, None, False,
                                      0x08000000 | 0x00000400, block, str(cwd),
                                      C.byref(startup), C.byref(process)))
        while True:
            wait = kernel.WaitForSingleObject(process.hProcess, 100)
            if wait == 0:
                break
            if wait != 258:
                raise C.WinError(C.get_last_error())
            observed.update(modules(kernel, process.hProcess))
            @C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
            def observe_window(window, _):
                pid = W.DWORD()
                user.GetWindowThreadProcessId(window, C.byref(pid))
                if pid.value == process.dwProcessId:
                    own_windows.add(window)
                return True

            C.set_last_error(0)
            if not user.EnumDesktopWindows(desktop, observe_window, 0) and C.get_last_error():
                raise C.WinError(C.get_last_error())
            if time.monotonic() - start > timeout:
                timed_out = True
                checked(kernel.TerminateProcess(process.hProcess, 0xDEAD))
                kernel.WaitForSingleObject(process.hProcess, 5000)
                break
        code = W.DWORD()
        checked(kernel.GetExitCodeProcess(process.hProcess, C.byref(code)))
        after = input_desktop_name(user)
        if before == name or after == name:
            raise RuntimeError("Private desktop isolation check failed")
        return {"exit_code": code.value, "exit_hex": f"0x{code.value:08X}",
                "timed_out": timed_out, "seconds": round(time.monotonic() - start, 2),
                "private_desktop_observed": bool(own_windows),
                "input_desktop_unchanged": before == after,
                "loaded_modules": sorted(observed, key=str.lower)}
    finally:
        if process.hProcess:
            if kernel.WaitForSingleObject(process.hProcess, 0) == 258:
                kernel.TerminateProcess(process.hProcess, 0xDEAD)
                kernel.WaitForSingleObject(process.hProcess, 5000)
            kernel.CloseHandle(process.hThread)
            kernel.CloseHandle(process.hProcess)
        kernel.SetErrorMode(previous_error_mode)
        user.CloseDesktop(desktop)


def clean_environment(root):
    keys = ("SYSTEMROOT", "WINDIR", "SYSTEMDRIVE", "PROGRAMDATA", "COMSPEC",
            "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "OS")
    current = {k.upper(): v for k, v in os.environ.items()}
    env = {key: current[key] for key in keys if key in current}
    windows = Path(env["SYSTEMROOT"])
    env["PATH"] = os.pathsep.join(str(p) for p in (windows / "System32", windows))
    # Fresh settings enable single-instance IPC, whose pipe name uses USERNAME.
    env["USERNAME"] = "LiteViewProbe" + uuid.uuid4().hex
    for key, folder in {"APPDATA": "roaming", "LOCALAPPDATA": "local", "TEMP": "temp",
                        "TMP": "temp", "USERPROFILE": "profile", "HOME": "profile"}.items():
        path = root / folder
        path.mkdir(exist_ok=True)
        env[key] = str(path)
    return env


def validate_result(record, log, screenshot, *, needs_image):
    from PIL import Image

    errors = []
    if record["timed_out"] or record["exit_code"] != 0:
        errors.append(f"Startup exited {record['exit_hex']} (timeout={record['timed_out']})")
    if not record["private_desktop_observed"] or not record["input_desktop_unchanged"]:
        errors.append("Desktop isolation was not confirmed")
    if "\u7b2c\u4e00\u5e27" not in log or "trace startup-complete" not in log:
        errors.append("App did not render its first frame and reach the replay checkpoint")
    if needs_image and "\u7b2c\u4e00\u5f20\u56fe\u663e\u793a" not in log:
        errors.append("No image decode evidence in app log")
    if not screenshot.is_file():
        errors.append("No app framebuffer screenshot")
    else:
        try:
            with Image.open(screenshot) as image:
                image.load()
                if image.width < 520 or image.height < 360 or all(lo == hi for lo, hi in image.convert("RGB").getextrema()):
                    errors.append("Blank or undersized app framebuffer")
                record["screenshot_size"] = list(image.size)
                if needs_image:
                    small = image.convert("RGB").resize((100, 100))
                    pixels = [small.getpixel((x, y)) for y in range(100) for x in range(100)]
                    sample_visible = all(
                        sum(max(abs(a - b) for a, b in zip(pixel, color)) <= 16 for pixel in pixels) >= 30
                        for color in [(26, 58, 96), (249, 193, 46), (212, 49, 61)]
                    )
                    record["sample_visible"] = sample_visible
                    if not sample_visible:
                        errors.append("Generated test image is not visible in the app framebuffer")
        except (OSError, SyntaxError, ValueError) as error:
            errors.append(f"Unreadable app framebuffer: {error}")
    record["passed"], record["errors"] = not errors, errors
    return record


def probe(binary, root, backend, scene, timeout, *, mode="configured", startup_state=None):
    """backend: "default" or a renderer forced via LITEVIEW_RENDERER ("d3d11" / "warp").
    startup_state: startup.json to seed (configured mode), e.g. a previous attempt that never drew a frame."""
    root.mkdir(parents=True, exist_ok=False)
    app, config = root / "app", root / "config"
    app.mkdir()
    config.mkdir()
    target = app / "LiteView.exe"
    shutil.copy2(binary, target)
    env = clean_environment(root)
    settings = {"language": "En", "single_instance": False, "ask_default_app": False,
                "fit_window": False, "preload": 0,
                "window": {"pos": [80, 80], "size": [800, 600], "maximized": False}}
    if startup_state is not None and mode != "configured":
        raise ValueError("startup_state needs the configured mode")
    if mode == "configured":
        (config / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
        if startup_state is not None:
            (config / "startup.json").write_text(json.dumps(startup_state), encoding="utf-8")
        env["LITEVIEW_CONFIG_DIR"] = str(config)
    elif mode == "fresh-portable":
        (app / "LiteView.portable").touch()
    elif mode != "fresh-installed":
        raise ValueError(f"Unknown configuration mode: {mode}")
    env["LITEVIEW_LOG"] = str(root / "app.log")
    steps = (["waitimage"] if scene != "empty" else []) + ["wait 1200", "trace startup-complete", "shot frame.png", "wait 300", "quit"]
    script = root / "replay.txt"
    script.write_text("\n".join(steps) + "\n", encoding="utf-8")
    env["LITEVIEW_SCRIPT"] = str(script)
    if backend != "default":
        env["LITEVIEW_RENDERER"] = backend
    args = [target]
    if scene != "empty":
        from PIL import Image, ImageDraw

        sample = root / f"sample.{scene}"
        image = Image.new("RGB", (640, 480), (26, 58, 96))
        draw = ImageDraw.Draw(image)
        draw.rectangle((80, 80, 560, 400), fill=(249, 193, 46))
        draw.ellipse((220, 140, 420, 340), fill=(212, 49, 61))
        image.save(sample)
        args.append(sample)
    record = {"backend": backend, "scene": scene, "mode": mode,
              "binary_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
              "environment": env}
    record.update(run_private(args, root, env, timeout))
    log_path = root / "app.log"
    log = log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
    validate_result(record, log, root / "frame.png", needs_image=scene != "empty")
    (root / "result.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New directory only")
    parser.add_argument("--backend", choices=("default", "d3d11", "warp"), default="default")
    parser.add_argument("--scene", choices=("empty", "png", "jpg", "webp"), default="empty")
    parser.add_argument("--mode", choices=("configured", "fresh-portable", "fresh-installed"), default="configured")
    parser.add_argument("--timeout", type=float, default=25)
    args = parser.parse_args()
    record = probe(args.binary.resolve(strict=True), args.output.resolve(), args.backend,
                   args.scene, args.timeout, mode=args.mode)
    print(json.dumps({k: v for k, v in record.items() if k not in ("loaded_modules", "environment")}, indent=2))
    raise SystemExit(0 if record["passed"] else 1)


if __name__ == "__main__":
    main()
