"""Generate authored ordered-heading witnesses with exact neighbor searches."""

from __future__ import annotations

import argparse
from fractions import Fraction
from pathlib import Path
import struct

import generate_storage_vectors as storage

_SAMPLES = (
    ("binary64", 0xBFE2B27FA48424B8, 0x4068BC3C4730FCB4, 0x3FE936A9000B996D),
    ("binary64", 0xC01E4F65E4501343, 0x40891E9DFCF336E2, 0xC075FF1145EAC19F),
    ("binary64", 0x4080DE691417F503, 0xC019FE8A56E3319D, 0x3FDB6EB81BD3455F),
    ("prior-store", 0xC01B0A187251B5D3, 0xC001B6B5CD6CB87C, 0xC02CCAC44C3C62F0),
    ("prior-store", 0x4019C61314A656A5, 0xC0767D5EBD817D6E, 0xC00AAD5A472D20DC),
    ("prior-store", 0xC02C45EAADBA317D, 0x405FDC6B150634EB, 0xC033520DBBE5C206),
    ("reassociate", 0xBFC30751EECE40C7, 0x3FF822C63B85821D, 0x40443A0054BDDF45),
    ("reassociate", 0xBFDD9850D16C4092, 0x402A4F1AA4266DAA, 0xBFEFF01CDE9793A6),
    ("reassociate", 0xC008C99CF59BDF38, 0x40661576BE1B9F4B, 0xC07E9A73D63511C0),
)


def _fraction(bits: int) -> Fraction:
    value = struct.unpack("<d", bits.to_bytes(8, "little"))[0]
    return Fraction(value)


def _extended(value: Fraction) -> Fraction:
    maximum = (0x7FFE << 63) | ((1 << 63) - 1)
    code = storage._nearest_code(
        abs(value), maximum, storage._extended_value, ties_away=False
    )
    result = storage._extended_value(code)
    return -result if value < 0 else result


def _binary64(value: Fraction) -> int:
    code = storage._nearest_code(
        abs(value),
        0x7FEFFFFFFFFFFFFF,
        lambda bits: storage._ieee_value(bits, 8),
        ties_away=False,
    )
    return code | (int(value < 0) << 63)


def _step(heading: int, turn: int, direction: str) -> int:
    product = _extended(_fraction(turn) * _fraction(0x400921FB54442D18))
    delta = _extended(product / 180)
    previous = _fraction(heading)
    result = previous + delta if direction == "add" else previous - delta
    return _binary64(_extended(result))


def _vectors() -> str:
    rows = ["# category\tdirection\theading\tturn1\tturn2\tfirst\tsecond\n"]
    for category, heading, first_turn, second_turn in _SAMPLES:
        for direction in ("add", "subtract"):
            sign = 0 if direction == "add" else 1 << 63
            first_turn_bits = first_turn ^ sign
            second_turn_bits = second_turn ^ sign
            first = _step(heading, first_turn_bits, direction)
            second = _step(first, second_turn_bits, direction)
            rows.append(
                f"{category}\t{direction}\t{heading:016x}\t"
                f"{first_turn_bits:016x}\t{second_turn_bits:016x}\t"
                f"{first:016x}\t{second:016x}\n"
            )
    return "".join(rows)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    target = Path(__file__).with_name("heading-vectors.tsv")
    content = _vectors()
    if options.check:
        if target.read_text() != content:
            raise SystemExit("heading vectors are out of date")
    else:
        target.write_text(content)


if __name__ == "__main__":
    main()
