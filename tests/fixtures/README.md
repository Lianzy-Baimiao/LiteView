# Animation regression fixtures

These tiny synthetic images were generated for LiteView with Pillow 12.3.0 and
are covered by the repository MIT license. They contain no external artwork.

- `animation.png`: six 8×6 RGBA APNG frames, changing 3×4 rectangle on a solid background.
- `poster.png`: the same animation with a separate magenta default/poster image, which is not a playback frame.
- `animation.webp`: lossless animated WebP, optimized frame regions.

Frame delays: 40, 60, 80, 100, 120, 140 ms. Loop count: 2.
Tests compare a one-frame cache against a full decode, including backward seeks,
last-frame export and APNG poster handling. Test execution needs no Python.
