"""Generate independent adjacent-value cases with exact fractions."""

from __future__ import annotations

import argparse
from collections.abc import Callable
from fractions import Fraction
import hashlib
from pathlib import Path
import struct


def _power_of_two(exponent: int) -> Fraction:
    if exponent >= 0:
        return Fraction(1 << exponent)
    return Fraction(1, 1 << -exponent)


def _real48_bytes(exponent: int, fraction: int, negative: bool) -> bytes:
    fraction_and_sign = fraction | (int(negative) << 39)
    return bytes([exponent + 129]) + fraction_and_sign.to_bytes(5, "little")


def _extended_bytes(exponent: int, fraction: int, negative: bool) -> bytes:
    significand = fraction | (int(exponent != 0) << 63)
    signed_exponent = exponent | (int(negative) << 15)
    return significand.to_bytes(8, "little") + signed_exponent.to_bytes(
        2, "little"
    )


def _ieee_value(bits: int, width: int) -> Fraction:
    code = "<f" if width == 4 else "<d"
    number = struct.unpack(code, bits.to_bytes(width, "little"))[0]
    return Fraction(number)


def _row(profile: str, negative: bool, value: Fraction, expected: bytes) -> str:
    return (
        f"{profile}\t{int(negative)}\t{value.numerator}\t"
        f"{value.denominator}\t{expected.hex()}\n"
    )


def _real48_value(code: int) -> Fraction:
    exponent = code >> 39
    if exponent == 0:
        return Fraction(0)
    fraction = code & ((1 << 39) - 1)
    return Fraction((1 << 39) + fraction) * _power_of_two(exponent - 168)


def _extended_value(code: int) -> Fraction:
    exponent = code >> 63
    fraction = code & ((1 << 63) - 1)
    if exponent == 0:
        return Fraction(fraction) * _power_of_two(-16445)
    return Fraction((1 << 63) + fraction) * _power_of_two(exponent - 16446)


def _nearest_code(
    value: Fraction,
    maximum: int,
    value_at: Callable[[int], Fraction],
    *,
    ties_away: bool,
) -> int:
    """Find neighbors by monotone search."""
    low, high = 0, maximum
    assert value_at(low) <= value < value_at(high)
    while low + 1 < high:
        middle = (low + high) // 2
        if value_at(middle) <= value:
            low = middle
        else:
            high = middle
    lower_distance = value - value_at(low)
    upper_distance = value_at(high) - value
    if lower_distance < upper_distance:
        return low
    if lower_distance > upper_distance:
        return high
    return high if ties_away or low & 1 else low


def _samples() -> list[Fraction]:
    """Build portable deterministic rational samples."""
    result = []
    for index in range(64):
        seed = hashlib.sha256(
            b"chemifree-numeric-v1:" + index.to_bytes(4, "little")
        ).digest()
        numerator = int.from_bytes(seed[:4], "little") % ((1 << 31) - 1) + 1
        denominator = int.from_bytes(seed[4:8], "little") % ((1 << 31) - 1) + 1
        exponent = int.from_bytes(seed[8:10], "little") % 161 - 80
        result.append(
            Fraction(numerator, denominator) * _power_of_two(exponent)
        )
    return result


def _sampled_vectors() -> list[str]:
    """Build fixed rational samples with independent neighbor search."""
    result: list[str] = []
    for value in _samples():
        real_code = _nearest_code(
            value, (1 << 47) - 1, _real48_value, ties_away=True
        )
        real_exponent = (real_code >> 39) - 129
        real_fraction = real_code & ((1 << 39) - 1)
        f32_code = _nearest_code(
            value, 0x7F7FFFFF, lambda n: _ieee_value(n, 4), ties_away=False
        )
        f64_code = _nearest_code(
            value,
            0x7FEFFFFFFFFFFFFF,
            lambda n: _ieee_value(n, 8),
            ties_away=False,
        )
        for negative in (False, True):
            result.append(
                _row(
                    "real48",
                    negative,
                    value,
                    _real48_bytes(real_exponent, real_fraction, negative),
                )
            )
            result.append(
                _row(
                    "binary32",
                    negative,
                    value,
                    (f32_code | (int(negative) << 31)).to_bytes(4, "little"),
                )
            )
            result.append(
                _row(
                    "binary64",
                    negative,
                    value,
                    (f64_code | (int(negative) << 63)).to_bytes(8, "little"),
                )
            )
    return result


def _extended_vectors() -> list[str]:
    """Construct extended neighbors and search for general rational values."""
    result: list[str] = []
    positions = (
        Fraction(0),
        Fraction(1, 4),
        Fraction(1, 2),
        Fraction(3, 4),
        Fraction(1),
    )
    for exponent in (-2, -1, 0, 1, 2):
        step = _power_of_two(exponent - 63)
        for fraction in (0, 1, 1 << 62, (1 << 63) - 2):
            lower = (Fraction(1 << 63) + fraction) * step
            for position in positions:
                value = lower + step * position
                up = position > Fraction(1, 2) or (
                    position == Fraction(1, 2) and fraction & 1 != 0
                )
                result.extend(
                    _row(
                        "extended80",
                        negative,
                        value,
                        _extended_bytes(
                            exponent + 16383, fraction + int(up), negative
                        ),
                    )
                    for negative in (False, True)
                )
    maximum = (0x7FFE << 63) | ((1 << 63) - 1)
    for value in _samples():
        code = _nearest_code(value, maximum, _extended_value, ties_away=False)
        exponent = code >> 63
        fraction = code & ((1 << 63) - 1)
        result.extend(
            _row(
                "extended80",
                negative,
                value,
                _extended_bytes(exponent, fraction, negative),
            )
            for negative in (False, True)
        )
    return result


def _vectors() -> str:
    """Build neighboring cases using each profile's tie rule."""
    result = ["# profile\tnegative\tnumerator\tdenominator\texpected_le_hex\n"]
    positions = (
        Fraction(0),
        Fraction(1, 4),
        Fraction(1, 2),
        Fraction(3, 4),
        Fraction(1),
    )
    fractions = (0, 1, 1 << 38, (1 << 39) - 2)
    for exponent in (-128, -127, -1, 0, 1, 125, 126):
        step = _power_of_two(exponent - 39)
        for fraction in fractions:
            lower = (Fraction(1 << 39) + fraction) * step
            upper = lower + step
            for position in positions:
                value = lower + (upper - lower) * position
                expected_fraction = (
                    fraction if position < Fraction(1, 2) else fraction + 1
                )
                for negative in (False, True):
                    expected = _real48_bytes(
                        exponent, expected_fraction, negative
                    )
                    result.append(_row("real48", negative, value, expected))

    cases = (
        (
            "binary32",
            4,
            (
                0,
                1,
                2,
                0x007FFFFE,
                0x00800000,
                0x3F7FFFFE,
                0x3F800000,
                0x3F800001,
                0x7F7FFFFD,
            ),
        ),
        (
            "binary64",
            8,
            (
                0,
                1,
                2,
                0x000FFFFFFFFFFFFE,
                0x0010000000000000,
                0x3FEFFFFFFFFFFFFE,
                0x3FF0000000000000,
                0x3FF0000000000001,
                0x7FEFFFFFFFFFFFFD,
            ),
        ),
    )
    for profile, width, lower_values in cases:
        for lower_bits in lower_values:
            lower = _ieee_value(lower_bits, width)
            upper = _ieee_value(lower_bits + 1, width)
            for position in positions:
                value = lower + (upper - lower) * position
                round_up = position > Fraction(1, 2) or (
                    position == Fraction(1, 2) and lower_bits & 1 != 0
                )
                selected = lower_bits + int(round_up)
                for negative in (False, True):
                    bits = selected | (int(negative) << (width * 8 - 1))
                    result.append(
                        _row(
                            profile,
                            negative,
                            value,
                            bits.to_bytes(width, "little"),
                        )
                    )
    # These independent fixed witnesses exercise a denominator that is not a
    # power of two; IEEE values were checked using Python's struct packing.
    non_dyadic = (
        ("real48", bytes.fromhex("80abaaaaaa2a")),
        ("binary32", (0x3F2AAAAB).to_bytes(4, "little")),
        ("binary64", (0x3FE5555555555555).to_bytes(8, "little")),
    )
    for profile, positive in non_dyadic:
        for negative in (False, True):
            expected_bytes = bytearray(positive)
            if negative:
                expected_bytes[-1] |= 0x80
            result.append(
                _row(profile, negative, Fraction(2, 3), bytes(expected_bytes))
            )
    result.extend(_sampled_vectors())
    result.extend(_extended_vectors())
    assert len(result) == 1179
    return "".join(result)


def main() -> None:
    """Write vectors or verify that the checked-in artifact is reproducible."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="verify the existing vectors"
    )
    args = parser.parse_args()
    path = Path(__file__).with_name("storage-vectors.tsv")
    expected = _vectors()
    if args.check:
        if path.read_text(encoding="utf-8") != expected:
            raise SystemExit("storage vectors are not reproducible")
    else:
        path.write_text(expected, encoding="utf-8")


if __name__ == "__main__":
    main()
