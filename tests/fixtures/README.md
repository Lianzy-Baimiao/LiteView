# Animation regression fixtures

These tiny synthetic images were generated for LiteView with Pillow 12.3.0 and
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
