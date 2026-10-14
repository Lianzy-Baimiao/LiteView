#!/usr/bin/env python3
"""Create portable Windows/Linux archives or a macOS .app bundle. Python 3.11+."""
import argparse
from pathlib import Path
import plistlib
import re
import shutil
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def package(binary: Path, platform: str, arch: str, version: str, output: Path) -> Path:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("invalid package version")
    if platform not in {"windows", "linux", "macos"} or arch not in {"x64", "arm64"}:
        raise ValueError("unsupported platform or architecture")
    if not binary.is_file():
        raise FileNotFoundError(binary)
    output.mkdir(parents=True, exist_ok=True)
    # Keep staging inside the explicitly selected output directory; no recursive cleanup.
    stage = Path(tempfile.mkdtemp(prefix=".stage-", dir=output))
    name = f"LiteView-v{version}-{platform}-{arch}"
    folder = stage / name
    folder.mkdir()
    if platform == "macos":
        contents = folder / "LiteView.app" / "Contents"
        executable = contents / "MacOS" / "LiteView"
        resources = contents / "Resources"
        resources.mkdir(parents=True)
        executable.parent.mkdir()
        shutil.copy2(ROOT / "assets/app.icns", resources / "app.icns")
        plist = {
            "CFBundleName": "LiteView", "CFBundleDisplayName": "LiteView",
            "CFBundleIdentifier": "io.github.lianzy-baimiao.liteview",
            "CFBundleExecutable": "LiteView", "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version.split("-")[0], "CFBundleVersion": version.split("-")[0],
            "CFBundleIconFile": "app.icns", "LSMinimumSystemVersion": "11.0",
            "NSHighResolutionCapable": True,
            "NSHumanReadableCopyright": "MIT License, Lianzy-Baimiao",
            "CFBundleDocumentTypes": [{"CFBundleTypeName": "Images", "CFBundleTypeRole": "Viewer",
                "LSHandlerRank": "Alternate", "LSItemContentTypes": ["public.image", "public.svg-image"]}],
        }
        with (contents / "Info.plist").open("wb") as f:
            plistlib.dump(plist, f)
    else:
        executable = folder / ("LiteView.exe" if platform == "windows" else "LiteView")
        if platform == "windows":
            (folder / "LiteView.portable").touch()
        else:
            shutil.copy2(ROOT / "assets/app.png", folder / "liteview.png")
            shutil.copy2(ROOT / "packaging/liteview.desktop", folder / "liteview.desktop")
    shutil.copy2(binary, executable)
    executable.chmod(0o755)
    for doc in ["LICENSE", "README.md", "THIRD-PARTY-NOTICES.md", "LICENSES.html"]:
        shutil.copy2(ROOT / doc, folder / doc)
    shutil.copy2(ROOT / "docs/INSTALL.md", folder / "INSTALL.md")
    if platform == "macos":
        # Keep attribution inside the app when users copy only the .app bundle.
        for doc in ["LICENSE", "THIRD-PARTY-NOTICES.md", "LICENSES.html"]:
            shutil.copy2(ROOT / doc, resources / doc)
    if platform == "windows":
        archive = output / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            for file in sorted(folder.rglob("*")):
                if file.is_file():
                    z.write(file, file.relative_to(stage))
    else:
        archive = output / f"{name}.tar.gz"
        executable_name = executable.relative_to(stage).as_posix()

        def portable_mode(info: tarfile.TarInfo) -> tarfile.TarInfo:
            # Windows chmod cannot set Unix execute bits; write them into the archive.
            info.mode = 0o755 if info.isdir() or info.name == executable_name else 0o644
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            return info

        with tarfile.open(archive, "w:gz") as tar:
            tar.add(folder, arcname=name, filter=portable_mode)
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--platform", choices=["windows", "linux", "macos"], required=True)
    parser.add_argument("--arch", choices=["x64", "arm64"], required=True)
    parser.add_argument("--version", default=tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"])
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    print(package(args.binary, args.platform, args.arch, args.version.removeprefix("v"), args.output))
