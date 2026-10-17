import tempfile
import unittest
from pathlib import Path

from wack_report import summarize


class WackReportTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.report = Path(self.directory.name) / "report.xml"

    def parse(self, text, code=0):
        self.report.write_text(text, encoding="utf-8")
        return summarize(self.report, code)

    def report_xml(self, tests=None, partial="FALSE"):
        if tests is None:
            tests = '<TEST OPTIONAL="FALSE"><RESULT>PASS</RESULT></TEST>'
        return f'<REPORT OVERALL_RESULT="PASS" PARTIAL_RUN="{partial}"><REQUIREMENTS><REQUIREMENT>{tests}</REQUIREMENT></REQUIREMENTS></REPORT>'

    def test_missing_report_never_passes(self):
        self.assertEqual(summarize(self.report, 0)["status"], "inconclusive")

    def test_exit_zero_without_result_never_passes(self):
        for xml in ("", "<REPORT", "<REPORT/>", "<REPORT><OVERALL_RESULT>PASS</OVERALL_RESULT></REPORT>"):
            with self.subTest(xml=xml):
                self.assertEqual(self.parse(xml)["status"], "inconclusive")

    def test_exact_pass_and_zero_required(self):
        xml = self.report_xml()
        self.assertEqual(self.parse(xml)["status"], "reported-pass")
        for code in (None, 1, -1):
            self.assertEqual(self.parse(xml, code)["status"], "inconclusive")

    def test_fail_preserved_even_with_zero_exit(self):
        self.assertEqual(self.parse('<REPORT OVERALL_RESULT="FAIL"/>')["status"], "fail")

    def test_unknown_and_duplicate_results_never_pass(self):
        for xml in ('<REPORT OVERALL_RESULT="INCOMPLETE"/>', '<REPORT OVERALL_RESULT="pass"/>',
                    '<REPORT OVERALL_RESULT="PASS"><TEST OVERALL_RESULT="PASS"/></REPORT>'):
            with self.subTest(xml=xml):
                self.assertEqual(self.parse(xml)["status"], "inconclusive")

    def test_observed_results_preserved_without_inventing_required_optional_schema(self):
        result = self.parse('<REPORT OVERALL_RESULT="PASS"><TEST RESULT="NOT RUN" TITLE="Example"/></REPORT>')
        self.assertEqual(result["observed_results"], [{"element": "TEST", "attribute": "RESULT",
                                                      "value": "NOT RUN", "title": "Example"}])
        self.assertEqual(result["status"], "inconclusive")

    def test_partial_and_missing_tests_never_pass(self):
        for xml in (self.report_xml(partial="TRUE"), self.report_xml(tests=""), '<REPORT OVERALL_RESULT="PASS"/>'):
            self.assertEqual(self.parse(xml)["status"], "inconclusive")

    def test_required_failure_overrides_overall_pass(self):
        xml = self.report_xml('<TEST OPTIONAL="FALSE"><RESULT>FAIL</RESULT></TEST>')
        self.assertEqual(self.parse(xml)["status"], "fail")

    def test_unknown_or_not_run_tests_never_pass(self):
        for optional, value in (("FALSE", "NOT RUN"), ("TRUE", "SKIPPED"), ("TRUE", "new-enum"), ("", "PASS")):
            xml = self.report_xml(f'<TEST OPTIONAL="{optional}"><RESULT>{value}</RESULT></TEST>')
            self.assertEqual(self.parse(xml)["status"], "inconclusive")

    def test_actual_report_preserves_optional_failure_and_messages(self):
        report = Path(__file__).resolve().parent.parent / 'docs/validation/wack-1.0.0-20261017/wack.xml'
        result = summarize(report, 0)
        self.assertEqual(result["status"], "reported-pass")
        self.assertEqual(result["test_counts"]["required"]["pass"], 13)
        self.assertEqual(result["test_counts"]["optional"]["pass"], 10)
        self.assertEqual(result["test_counts"]["optional"]["fail"], 1)
        failure = [test for test in result["tests"] if test["result"] == "FAIL"][0]
        self.assertEqual(failure["name"], "Blocked executables")
        self.assertEqual(len(failure["messages"]), 6)

    def test_dtd_rejected_in_utf8_and_utf16(self):
        xml = '<!DOCTYPE REPORT [<!ENTITY value "PASS">]><REPORT OVERALL_RESULT="&value;"/>'
        for encoding in ("utf-8", "utf-16"):
            with self.subTest(encoding=encoding):
                self.report.write_bytes(xml.encode(encoding))
                self.assertEqual(summarize(self.report, 0)["status"], "inconclusive")

    def test_utf16_report_supported(self):
        self.report.write_bytes(self.report_xml().encode("utf-16"))
        self.assertEqual(summarize(self.report, 0)["status"], "reported-pass")


if __name__ == "__main__":
    unittest.main()
