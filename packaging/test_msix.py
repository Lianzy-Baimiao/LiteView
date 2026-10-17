import importlib.util
from pathlib import Path
import re
import struct
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile

SPEC = importlib.util.spec_from_file_location("msix_packager", Path(__file__).with_name("msix.py"))
msix = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(msix)
NS = {"f": msix.FOUNDATION, "uap": "http://schemas.microsoft.com/appx/manifest/uap/windows10",
      "uap2": "http://schemas.microsoft.com/appx/manifest/uap/windows10/2",
      "uap3": "http://schemas.microsoft.com/appx/manifest/uap/windows10/3",
      "uap10": "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
      "rescap": "http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"}


class Msix(unittest.TestCase):
    def test_unbundled_runtime_is_rejected(self):
        output = msix.ROOT / "dist"
        output.mkdir(exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix="pe-test-", dir=output))
        data = bytearray(1024)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:70] = b"PE\0\0\x64\x86"
        struct.pack_into("<H", data, 70, 1)
        struct.pack_into("<H", data, 84, 240)
        struct.pack_into("<H", data, 88, 0x20B)
        struct.pack_into("<II", data, 208, 0x1000, 40)
        struct.pack_into("<IIII", data, 336, 512, 0x1000, 512, 512)
        struct.pack_into("<IIIII", data, 512, 0, 0, 0, 0x1040, 0)
        binary = directory / "fixture.exe"
        for library in ["kernel32.dll", "VCRUNTIME140.dll", "MSVCP140.dll", "libgcc_s_seh-1.dll"]:
            fixture = data.copy()
            fixture[576:576 + len(library) + 1] = library.encode("ascii") + b"\0"
            binary.write_bytes(fixture)
            if library == "kernel32.dll":
                self.assertEqual(msix.verify_runtime(binary), [library])
            else:
                with self.assertRaisesRegex(ValueError, "unbundled runtime"):
                    msix.verify_runtime(binary)

    def test_store_version_rules(self):
        for value in ["1.0.0.0", "1.2.3.0", "65535.65535.65535.0"]:
            self.assertEqual(msix.store_version(value), value)
        for value in ["0.1.2.0", "1.0.0", "1.0.0.1", "65536.0.0.0", "1.01.0.0", "../1.0.0.0", "1.0.0-beta.0"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                msix.store_version(value)

    def test_manifest_identity_and_runtime_contract(self):
        root = ET.parse(msix.TEMPLATE / "AppxManifest.xml").getroot()
        identity = root.find("f:Identity", NS)
        self.assertEqual(identity.attrib, {"Name": "ice.LiteView", "Publisher": "CN=56079E3F-5861-4D9C-B9C3-93F76224C42E", "Version": "1.0.0.0", "ProcessorArchitecture": "x64"})
        app = root.find("f:Applications/f:Application", NS)
        self.assertEqual(app.get("Id"), "LiteView")
        self.assertEqual(app.get("Executable"), "LiteView.exe")
        self.assertEqual(app.get(f"{{{NS['uap10']}}}RuntimeBehavior"), "packagedClassicApp")
        self.assertEqual(app.get(f"{{{NS['uap10']}}}TrustLevel"), "mediumIL")
        self.assertEqual([node.get("Name") for node in root.find("f:Capabilities", NS)], ["runFullTrust"])
        self.assertEqual(root.find("f:Dependencies/f:TargetDeviceFamily", NS).get("MinVersion"), "10.0.19041.0")

    def test_associations_match_application_and_quote_paths(self):
        root = ET.parse(msix.TEMPLATE / "AppxManifest.xml").getroot()
        source = (msix.ROOT / "src/platform/assoc.rs").read_text(encoding="utf-8")
        groups = source.split("pub const GROUPS:", 1)[1].split("= [", 1)[1].split("];", 1)[0]
        expected = re.findall(r'"([a-z0-9]+)"', groups)
        actual = [node.text.removeprefix(".") for node in root.findall(".//uap:FileType", NS)]
        self.assertEqual(actual, expected)
        association = root.find(".//uap3:FileTypeAssociation", NS)
        self.assertEqual(association.get("Parameters"), '"%1"')
        self.assertEqual(association.get("MultiSelectModel"), "Single")
        verbs = association.findall("uap2:SupportedVerbs/uap3:Verb", NS)
        self.assertEqual({v.get("Id"): v.get("Parameters") for v in verbs}, {"edit": '--edit "%1"', "mosaic": '--mosaic "%1"'})

    def test_localized_resources_and_all_icon_scales_exist(self):
        text = (msix.TEMPLATE / "AppxManifest.xml").read_text(encoding="utf-8")
        referenced = set(re.findall(r"ms-resource:([A-Za-z]+)", text))
        for lang in ["en-US", "zh-CN"]:
            resources = ET.parse(msix.TEMPLATE / "Strings" / lang / "Resources.resw")
            entries = {node.get("name"): node.findtext("value") for node in resources.findall("data")}
            self.assertEqual(set(entries), referenced)
            self.assertTrue(all(entries.values()))
        for name, size in [("StoreLogo", 50), ("Square44x44Logo", 44), ("Square150x150Logo", 150)]:
            for scale in [100, 125, 150, 200, 400]:
                data = (msix.TEMPLATE / "Assets" / f"{name}.scale-{scale}.png").read_bytes()
                self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
                px = (size * scale + 50) // 100
                self.assertEqual(struct.unpack(">II", data[16:24]), (px, px))

    def test_layout_is_whitelisted_and_rejects_non_x64(self):
        output = msix.ROOT / "dist"
        output.mkdir(exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix="msix-test-", dir=output))
        binary = directory / "input.exe"
        binary.write_bytes(b"not a PE")
        with self.assertRaises(ValueError):
            msix.prepare_layout(binary, "1.0.0.0", directory)
        header = bytearray(64)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 60, 64)
        binary.write_bytes(header + b"PE\0\0\x4c\x01")
        with self.assertRaises(ValueError):
            msix.verify_x64_binary(binary)
        binary.write_bytes(header + b"PE\0\0\x64\x86")
        (directory / "LiteView.portable").touch()
        (directory / "private.pfx").write_bytes(b"must not be packaged")
        stage = msix.prepare_layout(binary, "1.0.1.0", directory)
        self.assertEqual((stage / "LiteView.exe").read_bytes(), binary.read_bytes())
        self.assertFalse((stage / "LiteView.portable").exists())
        self.assertFalse((stage / "private.pfx").exists())
        self.assertTrue((stage / "PRIVACY.md").is_file())
        identity = ET.parse(stage / "AppxManifest.xml").getroot().find("f:Identity", NS)
        self.assertEqual(identity.get("Version"), "1.0.1.0")

    def test_package_verification_rejects_test_signatures_and_wrong_identity(self):
        directory = msix.ROOT / "dist"
        directory.mkdir(exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix="msix-verify-", dir=directory))
        for extra, version in [("AppxSignature.p7x", "1.0.0.0"), ("private.pfx", "1.0.0.0"), ("", "1.0.1.0")]:
            package = directory / "fixture.msix"
            with zipfile.ZipFile(package, "w") as z:
                z.writestr("AppxManifest.xml", (msix.TEMPLATE / "AppxManifest.xml").read_bytes())
                for name in ["AppxBlockMap.xml", "[Content_Types].xml", "LiteView.exe", "resources.pri", "LICENSES.html", "PRIVACY.md"]:
                    z.writestr(name, b"fixture")
                if extra:
                    z.writestr(extra, b"fixture")
            with self.assertRaises(ValueError):
                msix.verify_package(package, version)
        with zipfile.ZipFile(directory / "empty.msix", "w"):
            pass
        with self.assertRaises(ValueError):
            msix.verify_package(directory / "empty.msix", "1.0.0.0")


if __name__ == "__main__":
    unittest.main()
