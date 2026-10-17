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

    def test_missing_report_never_passes(self):
        self.assertEqual(summarize(self.report, 0)["status"], "inconclusive")

    def test_exit_zero_without_result_never_passes(self):
        for xml in ("", "<REPORT", "<REPORT/>", "<REPORT><OVERALL_RESULT>PASS</OVERALL_RESULT></REPORT>"):
            with self.subTest(xml=xml):
                self.assertEqual(self.parse(xml)["status"], "inconclusive")

    def test_exact_pass_and_zero_required(self):
        xml = '<REPORT OVERALL_RESULT="PASS"/>'
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
        self.assertIn("not-run", result["reason"])

    def test_dtd_rejected_in_utf8_and_utf16(self):
        xml = '<!DOCTYPE REPORT [<!ENTITY value "PASS">]><REPORT OVERALL_RESULT="&value;"/>'
        for encoding in ("utf-8", "utf-16"):
            with self.subTest(encoding=encoding):
                self.report.write_bytes(xml.encode(encoding))
                self.assertEqual(summarize(self.report, 0)["status"], "inconclusive")

    def test_utf16_report_supported(self):
        self.report.write_bytes('<REPORT OVERALL_RESULT="PASS"/>'.encode("utf-16"))
        self.assertEqual(summarize(self.report, 0)["status"], "reported-pass")


if __name__ == "__main__":
    unittest.main()
