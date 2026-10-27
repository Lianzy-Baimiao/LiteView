#!/usr/bin/env python3
"""Build an unsigned Store MSIX. Requires Windows SDK MakePri and MakeAppx; never installs it."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import tomllib
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "packaging/msix"
FOUNDATION = "http://schemas.microsoft.com/appx/manifest/foundation/windows10"
SCALES = [100, 125, 150, 200, 400]
# 开始菜单、任务栏、资源管理器、右键菜单按像素大小取 Square44x44Logo 的 targetsize 图；只有 scale 图时系统把 44 px 的图
# 缩小来用，图标发糊。unplated：任务栏等处不加底板
TARGET_SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]
ASSETS = [f"{name}.scale-{scale}.png" for name in ["StoreLogo", "Square44x44Logo", "Square150x150Logo"] for scale in SCALES]
ASSETS += [f"Square44x44Logo.targetsize-{size}{alt}.png" for size in TARGET_SIZES for alt in ["", "_altform-unplated"]]
LANGUAGES = ["en-US", "zh-CN"]


def store_version(value: str) -> str:
    if not re.fullmatch(r"[1-9][0-9]*\.[0-9]+\.[0-9]+\.0", value):
        raise ValueError("Store version needs four numeric parts: first > 0, fourth = 0")
    if any(int(part) > 65535 or str(int(part)) != part for part in value.split(".")):
        raise ValueError("Store version parts must be canonical integers <= 65535")
    return value


def verify_x64_binary(binary: Path) -> None:
    with binary.open("rb") as stream:
        dos = stream.read(64)
        if len(dos) < 64 or dos[:2] != b"MZ":
            raise ValueError("expected a Windows x64 PE executable")
        stream.seek(struct.unpack_from("<I", dos, 60)[0])
        header = stream.read(6)
        if header != b"PE\0\0\x64\x86":
            raise ValueError("the Store package currently supports x64 PE only")


def imported_dlls(binary: Path) -> list[str]:
    verify_x64_binary(binary)
    data = binary.read_bytes()
    pe = struct.unpack_from("<I", data, 60)[0]
    optional = pe + 24
    if struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise ValueError("expected PE32+ optional header")
    sections = optional + struct.unpack_from("<H", data, pe + 20)[0]
    count = struct.unpack_from("<H", data, pe + 6)[0]
    if count > 96:
        raise ValueError("invalid PE section count")

    def offset(rva: int) -> int:
        for index in range(count):
            base = sections + index * 40
            _, address, size, raw = struct.unpack_from("<IIII", data, base + 8)
            if address <= rva < address + size:
                result = raw + rva - address
                if result < len(data):
                    return result
        raise ValueError("PE import points outside file-backed sections")

    rva, size = struct.unpack_from("<II", data, optional + 120)
    if not rva:
        return []
    imports = set()
    for index in range(min(size // 20, 4096)):
        pos = offset(rva + index * 20)
        descriptor = struct.unpack_from("<IIIII", data, pos)
        if not any(descriptor):
            return sorted(imports)
        start = offset(descriptor[3])
        end = data.find(b"\0", start, min(start + 512, len(data)))
        if end < 0:
            raise ValueError("unterminated PE DLL name")
        imports.add(data[start:end].decode("ascii").lower())
    raise ValueError("unterminated PE import table")


def verify_runtime(binary: Path) -> list[str]:
    imports = imported_dlls(binary)
    redistributables = [name for name in imports if name.startswith(("vcruntime", "msvcp", "libgcc_", "libstdc++", "libwinpthread"))]
    if redistributables:
        raise ValueError(f"unbundled runtime DLLs {redistributables}; use the Windows release CRT-static build")
    return imports


def prepare_layout(binary: Path, version: str, output: Path) -> Path:
    store_version(version)
    verify_x64_binary(binary)
    # Whitelist payload files. Never sweep in portable markers, settings, keys or build tools.
    documents = [ROOT / name for name in ["LICENSE", "THIRD-PARTY-NOTICES.md", "LICENSES.html"]]
    documents += [ROOT / "docs/PRIVACY.md"]
    documents += [ROOT / "assets/licenses" / name for name in ["LIBWEBP-COPYING.txt", "LIBWEBP-PATENTS.txt"]]
    for doc in documents:
        if not doc.is_file():
            raise FileNotFoundError(doc)
    assets = [Path("Assets") / name for name in ASSETS]
    assets += [Path("Strings") / language / "Resources.resw" for language in LANGUAGES]
    for asset in assets:
        if not (TEMPLATE / asset).is_file():
            raise FileNotFoundError(TEMPLATE / asset)
    output.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".msix-stage-", dir=output))
    manifest = (TEMPLATE / "AppxManifest.xml").read_text(encoding="utf-8")
    manifest, replaced = re.subn(r'Version="1\.0\.0\.0"', f'Version="{version}"', manifest, count=1)
    if replaced != 1:
        raise ValueError("manifest template Store version was not found")
    (stage / "AppxManifest.xml").write_text(manifest, encoding="utf-8")
    shutil.copy2(binary, stage / "LiteView.exe")
    for asset in assets:
        (stage / asset).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(TEMPLATE / asset, stage / asset)
    for doc in documents:
        shutil.copy2(doc, stage / doc.name)
    return stage


def verify_package(package: Path, version: str) -> dict:
    with zipfile.ZipFile(package) as archive:
        names = set(archive.namelist())
        required = {"AppxManifest.xml", "AppxBlockMap.xml", "[Content_Types].xml", "LiteView.exe", "resources.pri", "LICENSES.html", "PRIVACY.md"}
        if not required <= names:
            raise ValueError(f"incomplete MSIX: missing {sorted(required - names)}")
        if "AppxSignature.p7x" in names:
            raise ValueError("Store output must be unsigned, not test-signed")
        if any(name.lower().endswith((".pfx", ".p12", ".key", ".portable", "settings.json")) for name in names):
            raise ValueError("unexpected private/portable content in MSIX")
        manifest = ET.fromstring(archive.read("AppxManifest.xml"))
        identity = manifest.find(f"{{{FOUNDATION}}}Identity")
        expected = ET.parse(TEMPLATE / "AppxManifest.xml").getroot().find(f"{{{FOUNDATION}}}Identity").attrib.copy()
        expected["Version"] = store_version(version)
        if identity is None or identity.attrib != expected:
            raise ValueError("package identity differs from the Store identity/version")
        return dict(identity.attrib)


def build(binary: Path, version: str, output: Path, makeappx: Path, makepri: Path) -> Path:
    store_version(version)
    binary, output = binary.resolve(), output.resolve()
    makeappx, makepri = makeappx.resolve(), makepri.resolve()
    imports = verify_runtime(binary)
    for tool in [makeappx, makepri]:
        if not tool.is_file():
            raise FileNotFoundError(tool)
    product_version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    subprocess.run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                    str(ROOT / "packaging/test_windows_resources.ps1"), "-Binary", str(binary), "-Version", product_version], check=True)
    package = output / f"LiteView-store-{version}-x64.msix"
    if package.exists() or package.with_suffix(".json").exists():
        raise FileExistsError(f"refusing to overwrite a versioned Store artifact: {package}")
    stage = prepare_layout(binary, version, output)
    config = stage.parent / f"{stage.name}-priconfig.xml"
    subprocess.run([str(makepri), "createconfig", "/cf", str(config), "/dq", "en-US", "/o"], check=True)
    # Keep every language and scale in this single package, not resource-pack sidecars.
    tree = ET.parse(config)
    for parent in tree.iter():
        for child in list(parent):
            if child.tag == "packaging":
                parent.remove(child)
    tree.write(config, encoding="utf-8", xml_declaration=True)
    subprocess.run([str(makepri), "new", "/pr", str(stage), "/cf", str(config), "/of", str(stage / "resources.pri"), "/o"], check=True)
    subprocess.run([str(makeappx), "pack", "/d", str(stage), "/p", str(package), "/h", "SHA256"], check=True)
    identity = verify_package(package, version)
    report = {
        "product_version": product_version,
        "store_id": "9PCG7D394JFC", "identity": identity, "signed": False,
        "sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "imported_dlls": imports,
        "layout": str(stage), "package": str(package),
        "status": "MakePri and MakeAppx passed; installation, WACK and Store review still required",
    }
    package.with_suffix(".json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return package


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--store-version", type=store_version, default="1.0.0.0")
    parser.add_argument("--makeappx", type=Path, required=True)
    parser.add_argument("--makepri", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "dist/store")
    args = parser.parse_args()
    print(build(args.binary, args.store_version, args.output, args.makeappx, args.makepri or args.makeappx.with_name("makepri.exe")))
