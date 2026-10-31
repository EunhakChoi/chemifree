"""Generate independent computer-point and metric length witnesses."""

from __future__ import annotations

import argparse
from fractions import Fraction
from pathlib import Path

import generate_storage_vectors as storage

_MILLIMETRES = {
    "point": Fraction(127, 360),
    "millimetre": Fraction(1),
    "centimetre": Fraction(10),
    "inch": Fraction(127, 5),
}
_MAGNITUDES = (
    0x0000000000000000,
    0x0000000000000001,
    0x000FFFFFFFFFFFFF,
    0x0010000000000000,
    0x3FB999999999999A,
    0x3FE0000000000000,
    0x3FF0000000000000,
    0x4039666666666666,
    0x4052000000000000,
    0x4330000000000001,
    0x7FDFFFFFFFFFFFFF,
    0x7FEFFFFFFFFFFFFF,
)


def _binary64(magnitude: Fraction, negative: bool) -> str:
    maximum = 0x7FEFFFFFFFFFFFFF
    last = storage._ieee_value(maximum, 8)
    previous = storage._ieee_value(maximum - 1, 8)
    if magnitude >= last + (last - previous) / 2:
        return "overflow"
    if magnitude >= last:
        code = maximum
    else:
        code = storage._nearest_code(
            magnitude,
            maximum,
            lambda bits: storage._ieee_value(bits, 8),
            ties_away=False,
        )
    return f"{code | (int(negative) << 63):016x}"


def _vectors() -> str:
    rows = ["# source\ttarget\tinput_bits\texpected_bits\n"]
    for source, source_mm in _MILLIMETRES.items():
        for target, target_mm in _MILLIMETRES.items():
            for bits in _MAGNITUDES:
                magnitude = storage._ieee_value(bits, 8) * source_mm / target_mm
                for negative in (False, True):
                    signed_bits = bits | (int(negative) << 63)
                    expected = _binary64(magnitude, negative)
                    rows.append(
                        f"{source}\t{target}\t{signed_bits:016x}\t{expected}\n"
                    )
    return "".join(rows)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    target = Path(__file__).with_name("unit-vectors.tsv")
    content = _vectors()
    if options.check:
        if target.read_text() != content:
            raise SystemExit("unit vectors are out of date")
    else:
        target.write_text(content)


if __name__ == "__main__":
    main()
