"""Unit guards; opt-in real-EXE tests use LITEVIEW_TEST_BINARY and a new output dir."""

import importlib.util
import json
import os
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import windows_runtime_audit as runtime
import windows_startup_probe as startup


class RuntimePolicyTests(unittest.TestCase):
    def test_does_not_treat_installed_redistributables_as_os_dependencies(self):
        for dll in ["VCRUNTIME140.dll", "MSVCP140.dll", "libgcc_s_seh-1.dll",
                    "libstdc++-6.dll", "libwinpthread-1.dll", "dxcompiler.dll", "vulkan-1.dll"]:
            self.assertFalse(runtime.is_system_dependency(dll), dll)
        # MSVCRT is the inbox legacy C runtime, not a VC++ redistributable DLL.
        for dll in ["KERNEL32.dll", "api-ms-win-core-synch-l1-2-0.dll", "d3dcompiler_47.dll", "msvcrt.dll", "WINSPOOL.DRV"]:
            self.assertTrue(runtime.is_system_dependency(dll), dll)
        self.assertFalse(runtime.is_system_dependency("C:\\private\\kernel32.dll"))

    def test_environment_drops_developer_paths_and_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.dict(os.environ, {"SYSTEMROOT": "C:\\Windows", "PATH": "C:\\SDK", "WGPU_BACKEND": "gl",
                                        "LITEVIEW_RENDERER": "warp", "TOKEN": "do-not-inherit"}, clear=True):
                env = startup.clean_environment(Path(directory))
            self.assertNotIn("TOKEN", env)
            self.assertNotIn("WGPU_BACKEND", env)
            self.assertNotIn("LITEVIEW_RENDERER", env)
            self.assertNotIn("SDK", env["PATH"])
            self.assertTrue(env["USERNAME"].startswith("LiteViewProbe"))
            for key in ["APPDATA", "LOCALAPPDATA", "TEMP", "USERPROFILE"]:
                self.assertTrue(Path(env[key]).is_relative_to(directory))


@unittest.skipUnless(importlib.util.find_spec("PIL"), "Pillow is needed for framebuffer fixtures")
class FramebufferTests(unittest.TestCase):
    def test_zero_exit_is_not_gui_success(self):
        with tempfile.TemporaryDirectory() as directory:
            record = {"timed_out": False, "exit_code": 0, "exit_hex": "0x00000000",
                      "private_desktop_observed": True, "input_desktop_unchanged": True}
            startup.validate_result(record, "", Path(directory) / "missing.png", needs_image=False)
            self.assertFalse(record["passed"])
            self.assertTrue(any("first frame" in error for error in record["errors"]))
            self.assertTrue(any("screenshot" in error for error in record["errors"]))

    def test_blank_or_wrong_frame_is_rejected(self):
        from PIL import Image, ImageDraw

        with tempfile.TemporaryDirectory() as directory:
            frame = Path(directory) / "frame.png"
            log = "\u7b2c\u4e00\u5e27; trace startup-complete; \u7b2c\u4e00\u5f20\u56fe\u663e\u793a"
            record = {"timed_out": False, "exit_code": 0, "exit_hex": "0x00000000",
                      "private_desktop_observed": True, "input_desktop_unchanged": True}
            image = Image.new("RGB", (800, 600), "white")
            image.save(frame)
            startup.validate_result(record, log, frame, needs_image=False)
            self.assertFalse(record["passed"])
            ImageDraw.Draw(image).rectangle((0, 0, 200, 200), fill="black")
            image.save(frame)
            startup.validate_result(record, log, frame, needs_image=True)
            self.assertFalse(record["passed"])
            self.assertFalse(record["sample_visible"])
            startup.validate_result(record, log, frame, needs_image=False)
            self.assertTrue(record["passed"])
            record.update(exit_code=0xC0000005, exit_hex="0xC0000005")
            startup.validate_result(record, log, frame, needs_image=False)
            self.assertFalse(record["passed"])


@unittest.skipUnless(os.name == "nt" and os.environ.get("LITEVIEW_TEST_BINARY"),
                     "Set LITEVIEW_TEST_BINARY for private-desktop integration tests")
class ReleasedStartupTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(os.environ["LITEVIEW_TEST_BINARY"]).resolve(strict=True)
        if "LITEVIEW_TEST_OUTPUT" in os.environ:
            cls.root = Path(os.environ["LITEVIEW_TEST_OUTPUT"]).resolve()
            cls.root.mkdir(parents=True, exist_ok=False)
        else:
            cls.root = Path(tempfile.mkdtemp(prefix="liteview-startup-"))

    def test_system_imports_and_empty_startup(self):
        report = runtime.audit(self.binary)
        (self.root / "runtime-imports.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        self.assertTrue(report["passed"], report["errors"])
        result = startup.probe(self.binary, self.root / "empty", "default", "empty", 25)
        self.assertTrue(result["passed"], result["errors"])

    def test_png_startup(self):
        result = startup.probe(self.binary, self.root / "png", "default", "png", 25)
        self.assertTrue(result["passed"], result["errors"])
        self.assertTrue(result["sample_visible"])
        # Windows draws with Direct3D 11 only: no Vulkan / Direct3D 12 runtime or driver gets loaded
        loaded = {Path(m).name.lower() for m in result["loaded_modules"]}
        self.assertFalse(loaded & {"vulkan-1.dll", "d3d12.dll", "d3d12core.dll"}, sorted(loaded))

    def test_missing_dll_is_detected(self):
        import pefile

        with pefile.PE(str(self.binary)) as pe:
            entry = next(e for e in pe.DIRECTORY_ENTRY_IMPORT if e.dll.lower() == b"kernel32.dll")
            offset = pe.get_offset_from_rva(entry.struct.Name)
        data = bytearray(self.binary.read_bytes())
        self.assertEqual(data[offset:offset + 12].lower(), b"kernel32.dll")
        data[offset:offset + 12] = b"missing0.dll"
        broken = self.root / "fault-injection-missing-dll.exe"
        broken.write_bytes(data)
        self.assertFalse(runtime.audit(broken)["passed"])
        result = startup.probe(broken, self.root / "missing-required", "default", "empty", 15)
        self.assertFalse(result["passed"])
        self.assertEqual(result["exit_code"], 0xC0000135)

    def test_warp_renders(self):
        result = startup.probe(self.binary, self.root / "warp", "warp", "png", 25)
        self.assertTrue(result["passed"], result["errors"])
        self.assertTrue(result["sample_visible"])
        log = (self.root / "warp/app.log").read_text(encoding="utf-8")
        self.assertIn(WARP_IN_USE, log)

    def test_hardware_start_without_first_frame_falls_back_to_warp(self):
        # A previous launch began with hardware Direct3D 11 and never drew a frame (driver crash or hang).
        version = tomllib.loads((Path(__file__).resolve().parents[1] / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
        seeded = {"version": version, "failed": [], "attempt": {"backend": "d3d11", "pid": 4294967292, "started": 1}, "last_failure": None}
        result = startup.probe(self.binary, self.root / "fallback", "default", "png", 25, startup_state=seeded)
        self.assertTrue(result["passed"], result["errors"])
        self.assertTrue(result["sample_visible"])
        log = (self.root / "fallback/app.log").read_text(encoding="utf-8")
        self.assertIn(WARP_IN_USE, log)
        state = json.loads((self.root / "fallback/config/startup.json").read_text(encoding="utf-8"))
        self.assertEqual(state["failed"], ["d3d11"])
        self.assertIsNone(state["attempt"])
        # Shown in Settings > About so a user can screenshot it
        self.assertIn("Direct3D 11", state["last_failure"])


# Display name of the renderer in use; the app log names it only on the line that reports it
WARP_IN_USE = "Direct3D 11 (WARP)"


if __name__ == "__main__":
    unittest.main()
