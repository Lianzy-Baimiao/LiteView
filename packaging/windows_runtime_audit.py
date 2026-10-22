"""Audit a single-file Windows release, including delay imports and API exports.

Developer tool: requires pefile. Resolves only reviewed OS DLLs, using the
Windows system-directory search flag; never loads the inspected executable.
Dynamic driver loading is covered separately by windows_startup_probe.py.
"""

import argparse
import ctypes as C
from ctypes import wintypes as W
import hashlib
import json
import os
from pathlib import Path


SYSTEM_DLLS = frozenset({
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "combase.dll",
    "comctl32.dll", "d3d11.dll", "d3d12.dll", "d3dcompiler_47.dll",
    "dwmapi.dll", "dxgi.dll", "gdi32.dll", "imm32.dll", "kernel32.dll",
    "msvcrt.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll", "opengl32.dll", "setupapi.dll",
    "shell32.dll", "shlwapi.dll", "uiautomationcore.dll", "user32.dll",
    "uxtheme.dll", "version.dll", "winmm.dll", "ws2_32.dll",
})


def is_system_dependency(name):
    name = name.lower()
    if any(char in name for char in "\\/:"):
        return False
    return name in SYSTEM_DLLS or (
        name.startswith(("api-ms-win-", "ext-ms-win-")) and name.endswith(".dll")
    )


def audit(binary):
    import pefile

    if os.name != "nt" or C.sizeof(C.c_void_p) != 8:
        raise RuntimeError("Requires 64-bit Windows Python")
    kernel = C.WinDLL("kernel32", use_last_error=True)
    kernel.LoadLibraryExW.argtypes = [W.LPCWSTR, W.HANDLE, W.DWORD]
    kernel.LoadLibraryExW.restype = W.HMODULE
    kernel.GetProcAddress.argtypes = [W.HMODULE, C.c_void_p]
    kernel.GetProcAddress.restype = C.c_void_p
    kernel.GetModuleFileNameW.argtypes = [W.HMODULE, W.LPWSTR, W.DWORD]
    kernel.GetModuleFileNameW.restype = W.DWORD
    kernel.FreeLibrary.argtypes = [W.HMODULE]
    kernel.FreeLibrary.restype = W.BOOL
    errors, entries = [], []
    with pefile.PE(str(binary)) as pe:
        if pe.FILE_HEADER.Machine != 0x8664 or pe.OPTIONAL_HEADER.Magic != 0x20B:
            raise ValueError("Expected Windows x64 PE32+ release")
        for kind, table in [("import", "DIRECTORY_ENTRY_IMPORT"), ("delay_import", "DIRECTORY_ENTRY_DELAY_IMPORT")]:
            for entry in getattr(pe, table, []):
                dll = entry.dll.decode("ascii").lower()
                record = {"dll": dll, "kind": kind, "symbols": len(entry.imports)}
                entries.append(record)
                if not is_system_dependency(dll):
                    errors.append(f"Unreviewed non-system dependency: {dll}")
                    continue
                handle = kernel.LoadLibraryExW(dll, None, 0x800)
                if not handle:
                    errors.append(f"Cannot resolve {dll} using System32 only: WinError {C.get_last_error()}")
                    continue
                try:
                    name = C.create_unicode_buffer(32768)
                    if kernel.GetModuleFileNameW(handle, name, len(name)):
                        record["resolved_to"] = name.value
                    for symbol in entry.imports:
                        address = C.cast(C.c_char_p(symbol.name), C.c_void_p) if symbol.name else C.c_void_p(symbol.ordinal)
                        if not kernel.GetProcAddress(handle, address):
                            label = symbol.name.decode("ascii") if symbol.name else f"ordinal {symbol.ordinal}"
                            errors.append(f"Missing OS API: {dll}!{label}")
                finally:
                    kernel.FreeLibrary(handle)
        delay = pe.OPTIONAL_HEADER.DATA_DIRECTORY[13]
        return {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "machine": "x64", "delay_directory_size": delay.Size,
                "dll_count": len({e['dll'] for e in entries}),
                "symbol_count": sum(e["symbols"] for e in entries),
                "imports": entries, "errors": errors, "passed": not errors}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = audit(args.binary.resolve(strict=True))
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
    print(json.dumps({k: v for k, v in result.items() if k != "imports"}, indent=2))
    raise SystemExit(0 if result["passed"] else 1)


if __name__ == "__main__":
    main()
