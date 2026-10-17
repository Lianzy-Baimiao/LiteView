"""Runner-only bootstrap launched by test_store_gui.ps1 inside the MSIX identity.

Uses the original release EXE and its own replay API, not OS-level input injection.
The bootstrap token is not identical to a real AUMID activation; both are checked
separately by the PowerShell entry point.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import traceback


FAMILY = "ice.LiteView_rz8m2sa64m4em"
AUMID = FAMILY + "!LiteView"


def replay(binary, output):
    from store_screenshots import RELEASE_SHA256, SCENES, capture, sha256

    if os.name != "nt" or sha256(binary) != RELEASE_SHA256:
        raise RuntimeError("Expected the unchanged Windows 1.0.0 release binary")
    output.mkdir(parents=True, exist_ok=False)
    diagnostics = output / "bootstrap-diagnostics.json"
    # No replay environment is injected until the diagnostics child has exited.
    env = {k: v for k, v in os.environ.items() if k not in ("LITEVIEW_SCRIPT", "LITEVIEW_CONFIG_DIR")}
    subprocess.run([str(binary), "--diagnostics", str(diagnostics)], env=env, timeout=30, check=True)
    identity = json.loads(diagnostics.read_text(encoding="utf-8"))
    if not identity["packaged"] or identity["package_family"] != FAMILY or identity["application_id"] != AUMID:
        raise RuntimeError("Replay child did not retain the expected package and application identity")
    config = Path(identity["config_dir"]).resolve(strict=True)
    if str(config) != str(Path(identity["package_local_folder"]).resolve(strict=True)):
        raise RuntimeError("Packaged configuration differs from the real WinRT LocalFolder")
    if config.name != "LocalState" or config.parent.name != FAMILY:
        raise RuntimeError("Refusing to change an unexpected configuration directory")
    result = {"status": "pass", "identity": identity, "binary_sha256": RELEASE_SHA256, "scenes": [],
              "scope": "Installed MSIX identity replay on a disposable runner; not Windows 10/11 human acceptance"}
    for scene in SCENES:
        result["scenes"].append(capture(binary, output, "en-us", scene, installed_config=config,
                                        minimum_size=(950, 650)))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--result", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = replay(args.binary.resolve(strict=True), args.output.resolve())
    except Exception:
        result = {"status": "fail", "error": traceback.format_exc()}
    # The PowerShell parent only reads a complete result, never a partially written JSON.
    pending = args.result.with_suffix(".pending.json")
    pending.write_text(json.dumps(result, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    pending.replace(args.result)
    raise SystemExit(0 if result["status"] == "pass" else 1)


if __name__ == "__main__":
    main()
