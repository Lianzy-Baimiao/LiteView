"""Conservative summary of a WACK report; never substitutes for raw-report review.

Microsoft TemplateStudio checks exit 0 AND the OVERALL_RESULT="PASS" attribute.
No stable, complete public WACK schema is assumed here. Unknown data fails closed.
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
    overall = result["overall_results"]
    if len(overall) != 1:
        result["reason"] = "Expected exactly one OVERALL_RESULT attribute"
    elif overall[0] == "FAIL":
        result["status"] = "fail"
        result["reason"] = "WACK reports overall FAIL; inspect raw required/optional tests"
    elif overall[0] == "PASS" and exit_code == 0:
        result["status"] = "reported-pass"
        result["reason"] = "WACK reports PASS and exit 0; raw warnings/not-run tests still require review"
    elif overall[0] == "PASS":
        result["reason"] = "PASS report conflicts with missing or nonzero process exit code"
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
