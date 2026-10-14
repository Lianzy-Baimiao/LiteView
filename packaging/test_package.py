import importlib.util
from pathlib import Path
import plistlib
import tarfile
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("packager", Path(__file__).with_name("package.py"))
packager = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packager)


class Packages(unittest.TestCase):
    def test_all_archive_layouts_and_macos_metadata(self):
        output_root = packager.ROOT / "dist"
        output_root.mkdir(exist_ok=True)
        output = Path(tempfile.mkdtemp(prefix="package-test-", dir=output_root))
        binary = output / "test-binary"
        binary.write_bytes(b"packaging layout test, not executable")
        for platform, arch in [("windows", "x64"), ("linux", "x64"), ("macos", "x64"), ("macos", "arm64")]:
            with self.subTest(platform=platform, arch=arch):
                archive = packager.package(binary, platform, arch, "0.1.0", output)
                prefix = f"LiteView-v0.1.0-{platform}-{arch}"
                if platform == "windows":
                    with zipfile.ZipFile(archive) as z:
                        self.assertIn(f"{prefix}/LiteView.portable", z.namelist())
                        self.assertEqual(z.read(f"{prefix}/LiteView.exe"), binary.read_bytes())
                        self.assertIn(f"{prefix}/LICENSES.html", z.namelist())
                else:
                    with tarfile.open(archive) as tar:
                        suffix = "LiteView.app/Contents/MacOS/LiteView" if platform == "macos" else "LiteView"
                        self.assertTrue(tar.getmember(f"{prefix}/{suffix}").mode & 0o111)
                        self.assertIn(f"{prefix}/LICENSES.html", tar.getnames())
                        self.assertNotIn(f"{prefix}/LiteView.portable", tar.getnames())
                        if platform == "macos":
                            self.assertIn(f"{prefix}/LiteView.app/Contents/Resources/LICENSES.html", tar.getnames())
                            plist = plistlib.load(tar.extractfile(f"{prefix}/LiteView.app/Contents/Info.plist"))
                            self.assertEqual(plist["CFBundleExecutable"], "LiteView")
                            self.assertEqual(plist["CFBundleShortVersionString"], "0.1.0")

    def test_rejects_unknown_platform_or_architecture(self):
        for platform, arch in [("unknown", "x64"), ("windows", "../arm64")]:
            with self.assertRaises(ValueError):
                packager.package(Path(__file__), platform, arch, "0.1.0", packager.ROOT / "dist")

    def test_rejects_path_in_version(self):
        with self.assertRaises(ValueError):
            packager.package(Path(__file__), "windows", "x64", "../bad", packager.ROOT / "dist")


if __name__ == "__main__":
    unittest.main()
