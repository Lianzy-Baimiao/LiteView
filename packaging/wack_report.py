"""Conservative summary of a WACK report; never substitutes for raw-report review.

Microsoft TemplateStudio checks exit 0 AND the OVERALL_RESULT="PASS" attribute.
Test classification follows the preserved 10.0.26100.8249 report, not a claimed
stable public schema. Unknown, missing and partial data fails closed.
"""

import argparse
import json
from pathlib import Path
import xml.etree.ElementTree as ET


MAX_REPORT_BYTES = 50 * 1024 * 1024


def summarize(report: Path, exit_code: int | None) -> dict:
    result = {
        "status": "inconclusive",
        "exit_code": exit_code,
        "overall_results": [],
        "observed_results": [],
        "tests": [],
        "test_counts": {kind: {status: 0 for status in ("pass", "fail", "not-run", "unknown")}
                        for kind in ("required", "optional", "unknown")},
        "reason": "",
        "scope": "Supplemental runner check, not Windows 10/11 acceptance or Store certification",
    }
    try:
        if report.stat().st_size > MAX_REPORT_BYTES:
            raise ValueError("report exceeds the 50 MiB limit")
        data = report.read_bytes()
        # NUL stripping also catches DTD declarations in UTF-16/32 reports.
        if b"<!DOCTYPE" in data.replace(b"\0", b"").upper():
            raise ValueError("DTD declarations are not accepted")
        root = ET.fromstring(data)
    except (OSError, ValueError, ET.ParseError) as error:
        result["reason"] = f"Missing or invalid XML report: {error}"
        return result
    for node in root.iter():
        for name, value in node.attrib.items():
            if name == "OVERALL_RESULT":
                result["overall_results"].append(value)
            elif name in ("RESULT", "STATUS"):
                result["observed_results"].append({
                    "element": node.tag,
                    "attribute": name,
                    "value": value,
                    "title": node.get("TITLE") or node.findtext("TITLE"),
                })
    result["partial_run"] = root.get("PARTIAL_RUN")
    for requirement in root.findall("./REQUIREMENTS/REQUIREMENT"):
        for test in requirement.findall("TEST"):
            kind = {"TRUE": "optional", "FALSE": "required"}.get(test.get("OPTIONAL"), "unknown")
            value = (test.findtext("RESULT") or "").strip()
            status = {"PASS": "pass", "FAIL": "fail", "NOT RUN": "not-run", "NOTRUN": "not-run",
                      "SKIPPED": "not-run", "NOT APPLICABLE": "not-run"}.get(value, "unknown")
            messages = [message.get("TEXT") or "".join(message.itertext())
                        for message in test.findall("./MESSAGES/MESSAGE")]
            result["tests"].append({"requirement": requirement.get("TITLE"), "name": test.get("NAME"),
                                    "kind": kind, "result": value, "messages": messages})
            result["test_counts"][kind][status] += 1
    overall = result["overall_results"]
    counts = result["test_counts"]
    if len(overall) != 1:
        result["reason"] = "Expected exactly one OVERALL_RESULT attribute"
    elif overall[0] == "FAIL" or counts["required"]["fail"]:
        result["status"] = "fail"
        result["reason"] = "WACK reports overall or required-test FAIL; inspect raw tests"
    elif overall[0] == "PASS":
        if exit_code != 0:
            result["reason"] = "PASS report conflicts with missing or nonzero process exit code"
        elif root.tag != "REPORT" or result["partial_run"] != "FALSE" or not result["tests"]:
            result["reason"] = "Missing test detail, unknown report shape, or partial run"
        elif any(counts["unknown"].values()) or any(c["unknown"] or c["not-run"] for c in counts.values()):
            result["reason"] = "Unknown or not-run tests require review; do not count them as passed"
        else:
            result["status"] = "reported-pass"
            result["reason"] = (f"WACK PASS, exit 0, {counts['required']['pass']} required tests passed; "
                                f"{counts['optional']['fail']} optional failures preserved for review")
    else:
        result["reason"] = f"Unknown overall result: {overall[0]!r}"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--exit-code", type=int)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = summarize(args.report, args.exit_code)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    print(f"WACK {result['status']}: {result['reason']}")
    raise SystemExit(0 if result["status"] == "reported-pass" else 1)


if __name__ == "__main__":
    main()
