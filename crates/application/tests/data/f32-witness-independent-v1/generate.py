"""Generate synthetic F32 midpoint fixtures using exact rational arithmetic.

The output is independent of the Rust reader. Redirect stdout to compare it
with vectors.tsv; no external or personal data is used.
"""

import fractions
import hashlib
import sys


def positive_value(bits: int) -> fractions.Fraction:
    """Return the exact value of a finite, nonnegative binary32 encoding."""
    exponent = (bits >> 23) & 0xFF
    fraction = bits & 0x7FFFFF
    if exponent == 0:
        return fractions.Fraction(fraction, 1 << 149)
    significand = (1 << 23) | fraction
    shift = exponent - 127 - 23
    if shift >= 0:
        return fractions.Fraction(significand << shift, 1)
    return fractions.Fraction(significand, 1 << -shift)


def exact_decimal(value: fractions.Fraction) -> str:
    """Render a rational with a power-of-two denominator without rounding."""
    sign = "-" if value < 0 else ""
    numerator = abs(value.numerator)
    denominator = value.denominator
    if denominator & (denominator - 1):
        raise ValueError("Expected a power-of-two denominator")
    places = denominator.bit_length() - 1
    digits = str(numerator * 5**places)
    if places:
        digits = digits.rjust(places + 1, "0")
        digits = f"{digits[:-places]}.{digits[-places:]}"
    return f"{sign}{digits}"


def main() -> None:
    """Write both signs and three exact neighbors around each midpoint."""
    starts = [0, 1, 0x007FFFFF, 0x00800000, 0x3F800000, 0x3F800001, 0x7F7FFFFE]
    for index in range(100):
        seed = f"chemifree-f32-fixture-v1-{index}".encode("ascii")
        starts.append(
            int.from_bytes(hashlib.sha256(seed).digest(), "big") % 0x7F7FFFFF
        )
    rows: list[str] = []
    for lower_bits in starts:
        lower = positive_value(lower_bits)
        upper = positive_value(lower_bits + 1)
        midpoint = (lower + upper) / 2
        epsilon = (upper - lower) / (1 << 35)
        tie_bits = lower_bits if lower_bits % 2 == 0 else lower_bits + 1
        for sign in (1, -1):
            cases = (
                (midpoint - epsilon, lower_bits),
                (midpoint, tie_bits),
                (midpoint + epsilon, lower_bits + 1),
            )
            for magnitude, bits in cases:
                signed_bits = bits | (0x80000000 if sign < 0 else 0)
                encoded = signed_bits.to_bytes(4, "little").hex()
                rows.append(f"{exact_decimal(sign * magnitude)}\t{encoded}\n")
    sys.stdout.write("".join(rows))


if __name__ == "__main__":
    main()
