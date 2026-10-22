"""Negative controls for verdicts: missing evidence must never become a pass."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

from windows_probe_checks import package_failures, validate_corpus_result


class PackageVerdictTests(unittest.TestCase):
    def result(self):
        return {"policy_exit": "0x00000000", "probe_exit": {"sample": "0x00000000"},
                "diagnostics_exit": "0x00000000", "nav_exit": "0x00000000",
                "gui": [{"file": "sample.heic", "status": "shown", "passed": True}],
                "diagnostics": {"packaged": True, "package_family": "family",
                                "application_id": "family!LiteView",
                                "config_dir": "C:\\Packages\\family\\LocalState",
                                "package_local_folder": "C:\\Packages\\family\\LocalState"}}

    def check(self, result, **overrides):
        return package_failures(result, **({"gui": True, "nav": True,
                                            "expected_family": "family"} | overrides))

    def test_executed_steps_require_explicit_zero(self):
        self.assertEqual(self.check(self.result()), [])
        for key in ("policy_exit", "diagnostics_exit", "nav_exit"):
            for value in (None, "0x00000011", "TIMEOUT"):
                with self.subTest(step=key, value=value):
                    result = self.result()
                    result[key] = value
                    self.assertIn(key, self.check(result))
            result = self.result()
            del result[key]
            self.assertIn(key, self.check(result))

    def test_unrequested_gui_steps_are_not_required(self):
        result = {"policy_exit": "0x00000000", "probe_exit": {"sample": "0x00000000"}}
        self.assertEqual(self.check(result, gui=False, nav=False, expected_family=None), [])

    def test_missing_or_wrong_identity_is_rejected(self):
        for key in ("packaged", "package_family", "application_id", "config_dir", "package_local_folder"):
            with self.subTest(key=key):
                result = self.result()
                result["diagnostics"][key] = None
                self.assertIn("identity", self.check(result))
        result = self.result()
        result["diagnostics"]["config_dir"] = None
        result["diagnostics"]["package_local_folder"] = None
        self.assertIn("identity", self.check(result))

    def test_empty_probes_and_unverified_gui_are_rejected(self):
        result = self.result()
        result["probe_exit"] = {}
        self.assertIn("probe_exit", self.check(result))
        result = self.result()
        result["gui"] = []
        self.assertIn("gui", self.check(result))
        for status in ("unverified", "CRASH/TIMEOUT", "error-page"):
            result = self.result()
            result["gui"][0].update(status=status, passed=False)
            self.assertIn("sample.heic", self.check(result))


@unittest.skipUnless(importlib.util.find_spec("PIL"), "Pillow is needed for framebuffer fixtures")
class CorpusVerdictTests(unittest.TestCase):
    def record(self):
        return {"exit_code": 0, "exit_hex": "0x00000000", "timed_out": False,
                "private_desktop_observed": True, "input_desktop_unchanged": True}

    def frame(self, root):
        from PIL import Image, ImageDraw

        path = Path(root) / "frame.png"
        image = Image.new("RGB", (800, 600), "white")
        ImageDraw.Draw(image).rectangle((200, 200, 500, 400), fill="black")
        image.save(path)
        return path

    def log(self, state):
        return ("\u7b2c\u4e00\u5e27\n"
                + ("\u7b2c\u4e00\u5f20\u56fe\u663e\u793a\n" if state == "shown" else "")
                + f"[ 200 ms INFO  liteview::devtools] image-state {state}\n"
                + "trace startup-complete\n")

    def test_zero_exit_and_checkpoint_without_completion_is_not_an_error_page(self):
        with tempfile.TemporaryDirectory() as directory:
            record = self.record()
            validate_corpus_result(record, "trace startup-complete", Path(directory) / "missing.png")
            self.assertFalse(record["passed"])
            self.assertNotEqual(record["status"], "error-page")

    def test_shown_and_error_require_completed_state_and_frame(self):
        with tempfile.TemporaryDirectory() as directory:
            frame = self.frame(directory)
            for state, status in (("shown", "shown"), ("error", "error-page")):
                result = validate_corpus_result(self.record(), self.log(state), frame)
                self.assertTrue(result["passed"], result)
                self.assertEqual(result["status"], status)
            result = validate_corpus_result(self.record(), self.log("error"), frame.with_name("missing.png"))
            self.assertFalse(result["passed"])
            frame.write_bytes(b"not a PNG")
            self.assertFalse(validate_corpus_result(self.record(), self.log("error"), frame)["passed"])

    def test_completion_after_checkpoint_is_too_late(self):
        with tempfile.TemporaryDirectory() as directory:
            log = "\u7b2c\u4e00\u5e27\ntrace startup-complete\n[INFO liteview::devtools] image-state error\n"
            self.assertFalse(validate_corpus_result(self.record(), log, self.frame(directory))["passed"])

    def test_legacy_no_image_is_unverified_even_with_frame(self):
        with tempfile.TemporaryDirectory() as directory:
            frame = self.frame(directory)
            log = "\u7b2c\u4e00\u5e27\ntrace startup-complete\n"
            result = validate_corpus_result(self.record(), log, frame, legacy=True)
            self.assertFalse(result["passed"])
            self.assertEqual(result["status"], "unverified")
            log = "\u7b2c\u4e00\u5f20\u56fe\u663e\u793a\n" + log
            self.assertTrue(validate_corpus_result(self.record(), log, frame, legacy=True)["passed"])

    def test_timeout_nonzero_and_desktop_mismatch_cannot_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            frame = self.frame(directory)
            for change in ({"timed_out": True}, {"exit_code": 1},
                           {"private_desktop_observed": False}, {"input_desktop_unchanged": False}):
                record = self.record() | change
                self.assertFalse(validate_corpus_result(record, self.log("error"), frame)["passed"])


if __name__ == "__main__":
    unittest.main()
