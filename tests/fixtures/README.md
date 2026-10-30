# Regression fixtures

These tiny synthetic images were generated for LiteView (the animations with Pillow 12.3.0) and
are covered by the repository MIT license. They contain no external artwork.

- `animation.png`: six 8×6 RGBA APNG frames, changing 3×4 rectangle on a solid background.
- `poster.png`: the same animation with a separate magenta default/poster image, which is not a playback frame.
- `animation.webp`: lossless animated WebP, optimized frame regions.
- `animation.jxl`: lossless JPEG XL with six composited frames and optimized frame regions.

Frame delays: 40, 60, 80, 100, 120, 140 ms. APNG/WebP loop count: 2.
JXL loops forever: cjxl 0.11.1 does not preserve the APNG loop count,
independently verified by decoding back to APNG with djxl 0.11.1.
Tests compare a one-frame cache against a full decode, including backward seeks,
last-frame export and APNG poster handling. Test execution needs no Python.

Generate the JXL fixture using the official libjxl v0.11.1 Windows tools:

```sh
cjxl tests/fixtures/animation.png tests/fixtures/animation.jxl -d 0 -e 7
```

JXL pixels and delays are also compared against the original APNG, not just
against the same decoder in full-cache mode.

`rotated.heic` (2026-10-28, pillow-heif 1.8.1 / libheif 1.23.6): a 300×200 picture, light grey with a red block
top-left and a blue block top-right, saved with EXIF orientation 6. libheif turns that into an `irot` box
(EXIF orientation reset to 1), so a correct viewer shows it as 200×300 with red top-right. The Windows test skips
when the HEIF / HEVC extensions are missing (CI). Script: `dist/heic-20261028/make.py` (not in the repository):

```python
im = Image.new("RGB", (300, 200), (230, 230, 230))
im.paste((220, 0, 0), (0, 0, 60, 60)); im.paste((0, 0, 220), (240, 0, 300, 60))
exif = Image.Exif(); exif[0x0112] = 6
im.save("rotated.heic", quality=90, exif=exif.tobytes())  # with pillow_heif.register_heif_opener()
```

`alpha.webm` (2026-10-30, ffmpeg n6.0 / libvpx-vp9 from LosslessCut): 64×48, 6 frames at 10 fps, a 20×20 red square
moving 5 px right per frame over a fully transparent background (VP9 with an alpha channel, like Telegram stickers).
The Windows test skips when the VP9 Video Extensions are missing (CI). ffmpeg decodes it to the same pixels (red 248 ~ 255).

```sh
ffmpeg -f lavfi -i "color=c=black@0.0:s=64x48:r=10:d=0.6,format=rgba" -f lavfi -i "color=c=red:s=20x20:r=10:d=0.6,format=rgba" \
  -filter_complex "[0][1]overlay=x='4+t*50':y=12:format=auto,format=yuva420p" \
  -c:v libvpx-vp9 -pix_fmt yuva420p -auto-alt-ref 0 -b:v 0 -crf 30 alpha.webm
```
