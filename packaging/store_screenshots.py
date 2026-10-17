"""Capture the released 1.0.0 UI using LiteView's own replay test interface.

Requires Pillow and a Windows desktop. Outputs are always created in a new
directory; only generated demonstration files are opened or overwritten.
This is not a packaged/MSIX activation test or an OS-level UI automation tool.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess

from PIL import Image, ImageDraw, ImageFont


RELEASE_SHA256 = "21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a"
SCENES = ("viewer", "editor", "redaction", "animation")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def font(size, bold=False):
    return ImageFont.truetype(str(Path(os.environ["WINDIR"]) / "Fonts" / ("segoeuib.ttf" if bold else "segoeui.ttf")), size)


def landscape(path):
    """Original geometric test artwork, not a photograph or an app mockup."""
    im = Image.new("RGB", (1440, 900))
    d = ImageDraw.Draw(im)
    for y in range(900):
        t = min(y / 590, 1)
        d.line((0, y, 1440, y), fill=tuple(round(a + (b - a) * t) for a, b in zip((223, 231, 218), (251, 217, 174))))
    d.ellipse((1000, 130, 1156, 286), fill="#f7aa62")
    d.polygon([(0, 526), (212, 258), (396, 486), (635, 195), (917, 515), (1178, 332), (1440, 522), (1440, 680), (0, 680)], fill="#648e8a")
    d.polygon([(395, 487), (635, 195), (603, 485), (718, 550)], fill="#3f7274")
    d.polygon([(521, 335), (635, 195), (760, 341), (669, 306), (620, 322), (589, 285)], fill="#eef0de")
    d.polygon([(97, 404), (212, 258), (335, 411), (237, 376), (207, 338), (186, 374)], fill="#e3e9d9")
    d.polygon([(0, 578), (142, 470), (341, 596), (458, 512), (706, 662), (921, 503), (1147, 570), (1440, 434), (1440, 730), (0, 730)], fill="#24595d")
    d.polygon([(0, 651), (334, 622), (738, 677), (1159, 615), (1440, 638), (1440, 900), (0, 900)], fill="#81b0ac")
    d.polygon([(0, 738), (445, 664), (689, 714), (420, 795), (968, 900), (0, 900)], fill="#4c9291")
    for x, y, w in [(804, 703, 190), (1036, 747, 241), (687, 791, 178), (913, 839, 253), (140, 840, 116)]:
        d.line((x, y, x + w, y), fill="#b9d2bc", width=3)
    d.polygon([(913, 730), (972, 730), (960, 742), (927, 742)], fill="#214c50")
    d.polygon([(945, 671), (945, 725), (918, 725)], fill="#fff3d7")
    d.line((947, 668, 947, 730), fill="#214c50", width=3)
    for x, y, s in [(19, 689, 1.2), (61, 708, 0.8), (1337, 613, 1.1), (1400, 604, 1.5), (1297, 621, 0.7)]:
        d.polygon([(x, y - 126 * s), (x - 33 * s, y - 31 * s), (x - 22 * s, y - 31 * s), (x - 45 * s, y), (x + 45 * s, y), (x + 22 * s, y - 31 * s), (x + 33 * s, y - 31 * s)], fill="#183f45")
    d.text((65, 63), "ALPINE / 01", fill="#30565a", font=font(22, True))
    d.text((65, 101), "A quiet afternoon", fill="#30565a", font=font(43))
    im.save(path)


def contact(path):
    im = Image.new("RGB", (1440, 900), "#e7e8e1")
    d = ImageDraw.Draw(im)
    d.rounded_rectangle((165, 106, 1288, 797), radius=28, fill="#d5d9d0")
    d.rounded_rectangle((155, 96, 1278, 787), radius=28, fill="#faf9f1")
    d.rounded_rectangle((155, 96, 1278, 237), radius=28, fill="#23575b")
    d.rectangle((155, 205, 1278, 237), fill="#23575b")
    d.text((210, 139), "CONTACT CARD", fill="#ffffff", font=font(36, True))
    d.text((990, 152), "DEMO ONLY", fill="#c0d6cc", font=font(22, True))
    for label, value, y in [("NAME", "Alex Chen", 292), ("PHONE", "000 0000 0000", 421), ("EMAIL", "alex@example.com", 550)]:
        d.text((217, y), label, fill="#6a8079", font=font(20, True))
        d.text((390, y - 12), value, fill="#213e43", font=font(40))
        d.line((217, y + 66, 1213, y + 66), fill="#e0e3d8", width=2)
    d.text((217, 698), "Fictional contact / Reserved example.com address", fill="#6a8079", font=font(23))
    im.save(path)


def animation(path):
    frames = []
    for frame in range(48):
        im = Image.new("RGB", (960, 600), "#122f36")
        d = ImageDraw.Draw(im)
        d.text((50, 40), "ORBIT / MOTION STUDY", fill="#b4c9bd", font=font(20, True))
        d.text((50, 76), "A little movement.", fill="#f4e7c9", font=font(34))
        for radius in (84, 150, 216):
            d.ellipse((480 - radius, 340 - radius, 480 + radius, 340 + radius), outline="#386069", width=2)
        d.ellipse((445, 305, 515, 375), fill="#edab70")
        for i, (radius, size, color) in enumerate([(84, 12, "#dce9c9"), (150, 20, "#78b8ab"), (216, 15, "#e8cd9a")]):
            a = 2 * math.pi * (frame / 48 + i / 3)
            x, y = 480 + radius * math.cos(a), 340 + radius * math.sin(a)
            d.ellipse((x - size, y - size, x + size, y + size), fill=color)
        d.text((736, 542), f"{frame + 1:02d} / 48", fill="#b4c9bd", font=font(22))
        frames.append(im)
    frames[0].save(path, save_all=True, append_images=frames[1:], duration=80, loop=0, optimize=False)


def capture(binary, root, language, scene):
    directory = root / language / scene
    directory.mkdir(parents=True)
    config = directory / "config"
    config.mkdir()
    settings = {
        "language": "ZhCn" if language == "zh-cn" else "En", "theme": "Dark",
        "single_instance": False, "ask_default_app": False, "fit_window": False,
        "smooth_zoom": False, "preload": 0, "anim_autoplay": False,
        "window": {"pos": [80, 80], "size": [1200, 780], "maximized": False},
        "redact_mode": "Rect", "redact_effect": "Mosaic", "mosaic_block": 22,
        "shape": "Arrow", "shape_width": 6, "color1": [245, 138, 69, 255],
        "text_size": 42, "text_bold": False, "confirm_overwrite": False,
    }
    (config / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    image = directory / ("Orbit.gif" if scene == "animation" else "Contact-demo.png" if scene == "redaction" else "Alpine-lake.png")
    {"animation": animation, "redaction": contact}.get(scene, landscape)(image)
    original_hash = sha256(image)
    screenshot = directory / f"{scene}.png"
    steps = ["waitimage", "wait 1200"]
    args = [str(binary)]
    if scene == "editor":
        args.append("--edit")
        label = "\u8fd9\u91cc\u8fd8\u6709\u96ea" if language == "zh-cn" else "Snow on the ridge"
        steps += ["key t", "wait 450", "click 690 345", f"text {label}", "wait 350", "key escape", "wait 200", "key u", "wait 200", "drag 725 395 465 323 20", "wait 350"]
    elif scene == "redaction":
        args.append("--mosaic")
        steps += ["drag 345 397 615 438 15", "wait 200", "drag 345 492 625 531 20", "wait 350"]
    elif scene == "animation":
        steps += ["key period", "key period", "key period", "wait 350"]
    steps += ["move 600 725", "wait 250", f"shot {screenshot.as_posix()}", "wait 350"]
    if scene in ("editor", "redaction"):
        steps += ["key ctrl+s", "wait 1800"]
    steps.append("quit")
    script = directory / "replay.txt"
    script.write_text("\n".join(steps) + "\n", encoding="utf-8")
    env = {**os.environ, "LITEVIEW_CONFIG_DIR": str(config), "LITEVIEW_SCRIPT": str(script)}
    args.append(str(image))
    # This calls the app-owned test harness; it does not send OS-level input.
    with (directory / "process.log").open("w", encoding="utf-8") as log:
        result = subprocess.run(args, env=env, cwd=directory, stdout=log, stderr=subprocess.STDOUT, timeout=45, check=True)
    with Image.open(screenshot) as shot:
        uniform = all(low == high for low, high in shot.convert("RGB").getextrema())
        if shot.width < 1366 or shot.height < 768 or uniform:
            raise RuntimeError(f"Invalid screenshot dimensions or blank image: {screenshot}")
        dimensions = list(shot.size)
    if scene in ("editor", "redaction") and sha256(image) == original_hash:
        raise RuntimeError(f"Editing did not change the generated sample: {scene}")
    return {"language": language, "scene": scene, "path": str(screenshot.relative_to(root)), "dimensions": dimensions, "sha256": sha256(screenshot), "source_before": original_hash, "source_after": sha256(image), "exit_code": result.returncode}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New output directory; existing paths are rejected")
    parser.add_argument("--language", choices=("zh-cn", "en-us", "both"), default="both")
    parser.add_argument("--scene", choices=(*SCENES, "all"), default="all")
    args = parser.parse_args()
    binary, root = args.binary.resolve(strict=True), args.output.resolve()
    if sha256(binary) != RELEASE_SHA256:
        raise SystemExit("Expected the verified public v1.0.0 Windows x64 executable.")
    root.mkdir(parents=True, exist_ok=False)
    manifest = {"version": "1.0.0", "binary_sha256": RELEASE_SHA256, "capture": "Actual client frame via LITEVIEW_SCRIPT; unpackaged binary identical to the MSIX payload; not MSIX GUI validation", "screenshots": []}
    languages = ("zh-cn", "en-us") if args.language == "both" else (args.language,)
    scenes = SCENES if args.scene == "all" else (args.scene,)
    for language in languages:
        for scene in scenes:
            manifest["screenshots"].append(capture(binary, root, language, scene))
            (root / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
            print(f"Captured {language}/{scene}", flush=True)


if __name__ == "__main__":
    main()
