"""Opt-in GUI tests for the current build's completed-load replay protocol."""

import os
import json
from pathlib import Path
import tempfile
import unittest

from windows_corpus_probe import gui
from windows_probe_checks import validate_corpus_result
from windows_startup_probe import clean_environment, run_private


@unittest.skipUnless(os.name == "nt" and os.environ.get("LITEVIEW_CORPUS_BINARY"),
                     "Set LITEVIEW_CORPUS_BINARY to a build with waitloaded support")
class CompletedLoadTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(os.environ["LITEVIEW_CORPUS_BINARY"]).resolve(strict=True)
        cls.root = Path(os.environ["LITEVIEW_CORPUS_OUTPUT"]).resolve() if "LITEVIEW_CORPUS_OUTPUT" in os.environ else Path(tempfile.mkdtemp()) / "corpus"
        cls.root.mkdir(parents=True, exist_ok=False)

    def test_shown_and_decode_error_are_distinct_completed_states(self):
        from PIL import Image

        image = self.root / "valid.png"
        Image.new("RGB", (640, 480), (212, 49, 61)).save(image)
        broken = self.root / "truncated.png"
        broken.write_bytes(b"\x89PNG\r\n\x1a\n")
        rows = gui(self.binary, [image, broken], self.root / "replays")
        self.assertEqual([row["status"] for row in rows], ["shown", "error-page"])
        self.assertTrue(all(row["passed"] for row in rows), rows)

    def test_pending_load_times_out_instead_of_becoming_an_error_page(self):
        root = self.root / "pending"
        root.mkdir()
        env = clean_environment(root)
        config = root / "config"
        config.mkdir()
        (config / "settings.json").write_text('{"single_instance":false,"ask_default_app":false}', encoding="utf-8")
        env["LITEVIEW_CONFIG_DIR"] = str(config)
        env["LITEVIEW_LOG"] = str(root / "app.log")
        script = root / "replay.txt"
        script.write_text("waitloaded\ntrace startup-complete\nshot frame.png\nquit\n", encoding="utf-8")
        env["LITEVIEW_SCRIPT"] = str(script)
        # No selected file: no completed load can ever be reported.
        record = run_private([self.binary], root, env, 2)
        log_path = root / "app.log"
        log = log_path.read_text(encoding="utf-8") if log_path.exists() else ""
        validate_corpus_result(record, log, root / "frame.png")
        (root / "result.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
        self.assertTrue(record["timed_out"])
        self.assertFalse(record["passed"])
        self.assertNotIn("trace startup-complete", log)


if __name__ == "__main__":
    unittest.main()
