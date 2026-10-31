# Independent binary32 midpoint fixtures

`vectors.tsv` contains 642 synthetic decimal inputs and expected little-endian
binary32 bits. The generator uses Python's exact `fractions.Fraction`, not the
Rust reader or a floating-point conversion: for seven boundary encodings and
100 finite encodings selected from a deterministic SHA-256 series it constructs
the exact midpoint to the next value, its two neighbors at an offset of one
ULP / 2^35, and both signs. Exact midpoint results use ties-to-even. This
includes zero, subnormal, normal and near-maximum values, and exposes a
double-rounding path through F64.

Regenerate and compare without changing the committed fixture:

```sh
python3 generate.py | cmp vectors.tsv -
```
