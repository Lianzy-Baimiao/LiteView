"""Replay local image fixtures on private desktops; no package installation.

New builds wait for an explicit completed image/error state. With --legacy-release,
old EXEs can verify shown images but missing image evidence stays unverified,
never a presumed error page. This is not a pixel-correctness or native-crash sandbox.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil

from windows_probe_checks import validate_corpus_result
from windows_startup_probe import clean_environment, run_private


def gui(exe, files, output, mode="configured", *, in_place=False, legacy=False, language="En", timeout=30):
    exe = Path(exe).resolve(strict=True)
    files = [Path(path).resolve(strict=True) for path in files]
    if not files:
        raise ValueError("No corpus files selected")
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary_hash = hashlib.sha256(exe.read_bytes()).hexdigest()
    target = exe
    if not in_place:
        (output / "app").mkdir()
        target = output / "app" / "LiteView.exe"
        shutil.copy2(exe, target)
    rows = []
    for index, source in enumerate(files):
        root = output / f"{index:03d}"
        root.mkdir()
        config = root / "config"
        config.mkdir()
        env = clean_environment(root)
        if mode == "configured":
            settings = {"language": language, "single_instance": False, "ask_default_app": False,
                        "preload": 0, "fit_window": False,
                        "window": {"pos": [80, 80], "size": [800, 600], "maximized": False}}
            (config / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
            env["LITEVIEW_CONFIG_DIR"] = str(config)
        elif mode != "fresh-installed" or in_place:
            raise ValueError("In-place tests require an isolated configuration")
        env["LITEVIEW_LOG"] = str(root / "app.log")
        env["LITEVIEW_LOG_LEVEL"] = "debug"
        script = root / "replay.txt"
        wait = "wait 2500" if legacy else "waitloaded\nwait 200"
        script.write_text(wait + "\ntrace startup-complete\nshot frame.png\nwait 100\nquit\n", encoding="utf-8")
        env["LITEVIEW_SCRIPT"] = str(script)
        sample_dir = root / "sample"
        sample_dir.mkdir()
        sample = sample_dir / source.name
        shutil.copy2(source, sample)
        record = {"file": source.name, "sample_sha256": hashlib.sha256(sample.read_bytes()).hexdigest(),
                  "binary_sha256": binary_hash, "legacy": legacy}
        record.update(run_private([target, sample], root, env, timeout))
        log_path = root / "app.log"
        log = log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
        validate_corpus_result(record, log, root / "frame.png", legacy=legacy)
        (root / "result.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
        row = {key: value for key, value in record.items() if key != "loaded_modules"}
        rows.append(row)
        print(f"{row['status']:14} {row['exit_hex']} {row['seconds']:5}s {source.name}", flush=True)
    (output / "summary.json").write_text(json.dumps(rows, indent=2), encoding="utf-8")
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New directory only")
    parser.add_argument("--legacy-release", action="store_true")
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    files = sorted(p for p in args.corpus.iterdir() if p.suffix.lower() in (".heic", ".heif", ".avif", ".png", ".jpg", ".webp"))
    rows = gui(args.binary, files, args.output, legacy=args.legacy_release, timeout=args.timeout)
    raise SystemExit(0 if all(row["passed"] for row in rows) else 1)


if __name__ == "__main__":
    main()
