"""Shared verdicts for developer-only Windows package and corpus probes."""

import re

from windows_startup_probe import validate_result


def package_failures(result, *, gui, nav, expected_family=None):
    required = ["policy_exit"]
    if gui:
        required.append("diagnostics_exit")
    if nav:
        required.append("nav_exit")
    bad = [key for key in required if result.get(key) != "0x00000000"]
    probes = result.get("probe_exit")
    if not probes:
        bad.append("probe_exit")
    bad += [key for key, value in (probes or {}).items()
           if value != "0x00000000"]
    if gui:
        rows = result.get("gui")
        if not rows:
            bad.append("gui")
        bad += [row["file"] for row in (rows or [])
                if row.get("passed") is not True or row.get("status") not in ("shown", "error-page")]
        identity = result.get("diagnostics") or {}
        valid = (identity.get("packaged") is True and bool(identity.get("package_family"))
                 and bool(identity.get("application_id")))
        if expected_family:
            valid = (valid and identity.get("package_family") == expected_family
                     and identity.get("application_id") == expected_family + "!LiteView"
                     and bool(identity.get("package_local_folder"))
                     and identity.get("config_dir") == identity.get("package_local_folder"))
        if not valid:
            bad.append("identity")
    elif expected_family:
        bad.append("identity")  # No diagnostics ran; a requested identity is not verified.
    return bad


def validate_corpus_result(record, log, screenshot, *, legacy=False):
    validate_result(record, log, screenshot, needs_image=False)
    checkpoint = log.find("trace startup-complete")
    before = log[:checkpoint] if checkpoint >= 0 else ""
    states = re.findall(r"liteview::devtools\] image-state (shown|error)(?:\r?\n|$)", before)
    shown = "\u7b2c\u4e00\u5f20\u56fe\u663e\u793a" in before
    state = states[-1] if states else ("shown" if legacy and shown else None)
    if state is None:
        record["errors"].append("No completed image/error state before the checkpoint")
    elif state == "shown" and not shown:
        record["errors"].append("Completed decode did not paint the image")
    record["passed"] = not record["errors"]
    if record["timed_out"] or record["exit_code"] != 0:
        record["status"] = "CRASH/TIMEOUT"
    elif not record["passed"]:
        record["status"] = "unverified"
    else:
        record["status"] = "shown" if state == "shown" else "error-page"
    return record
