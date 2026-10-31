"""Generate independent binary64 formatting witnesses."""

from __future__ import annotations

import argparse
from fractions import Fraction
import hashlib
from pathlib import Path
import struct

import generate_storage_vectors as storage

_SPECIAL = (
    0x0000000000000000,
    0x8000000000000000,
    0x0000000000000001,
    0x8000000000000001,
    0x3FF0000000000000,
    0xBFF0000000000000,
    0x3FF1000000000000,
    0xBFF1000000000000,
    0x3FEFFFFFFFFFFFFF,
    0x4025000000000000,
    0x7FEFFFFFFFFFFFFF,
    0xFFEFFFFFFFFFFFFF,
)


def _samples() -> list[int]:
    values = list(_SPECIAL)
    boundaries = (
        999999.5,
        -999999.5,
        0.00009999996,
        0.000099999,
        2.125,
        -2.125,
        2.375,
        -2.375,
        1.015625,
        -1.015625,
        1.046875,
        -1.046875,
    )
    values.extend(
        int.from_bytes(struct.pack("<d", number), "little")
        for number in boundaries
    )
    for index in range(100):
        seed = hashlib.sha256(
            b"chemifree-display-v1:" + index.to_bytes(4, "little")
        ).digest()
        magnitude = int.from_bytes(seed[:8], "little") & 0x7FEFFFFFFFFFFFFF
        values.extend((magnitude, magnitude | (1 << 63)))
    return values


def _float(bits: int) -> float:
    return float(struct.unpack("<d", bits.to_bytes(8, "little"))[0])


def _native_three(bits: int) -> int | None:
    number = _float(bits)
    magnitude = abs(Fraction(number))
    scaled = magnitude * 1000
    whole, remainder = divmod(scaled.numerator, scaled.denominator)
    if 2 * remainder >= scaled.denominator:
        whole += 1
    rounded = Fraction(whole, 1000)
    max_code = 0x7F7FFFFF
    max_value = storage._ieee_value(max_code, 4)
    previous = storage._ieee_value(max_code - 1, 4)
    if rounded >= max_value + (max_value - previous) / 2:
        return None
    if rounded >= max_value:
        code = max_code
    else:
        code = storage._nearest_code(
            rounded,
            max_code,
            lambda value: storage._ieee_value(value, 4),
            ties_away=False,
        )
    return code | ((bits >> 63) << 31)


def _vectors() -> str:
    rows = ["# profile\tbits\tplaces\texpected\n"]
    for bits in _samples():
        number = _float(bits)
        rows.append(f"default6\t{bits:016x}\t-\t{number:.6g}\n")
        rows.extend(
            f"fixed\t{bits:016x}\t{places}\t{number:.{places}f}\n"
            for places in range(9)
        )
        native = _native_three(bits)
        expected = "overflow" if native is None else f"{native:08x}"
        rows.append(f"native3\t{bits:016x}\t-\t{expected}\n")
    return "".join(rows)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    target = Path(__file__).with_name("display-vectors.tsv")
    content = _vectors()
    if options.check:
        if target.read_text() != content:
            raise SystemExit("display vectors are out of date")
    else:
        target.write_text(content)


if __name__ == "__main__":
    main()
