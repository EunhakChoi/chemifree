"""Find synthetic Real48 scale round trips that lose a stored bit."""

from fractions import Fraction
import sys

import generate_storage_vectors as storage

MAXIMUM = (1 << 47) - 1


def nearest(value: Fraction) -> tuple[int, Fraction]:
    code = storage._nearest_code(
        value, MAXIMUM, storage._real48_value, ties_away=True
    )
    return code, storage._real48_value(code)


def raw(code: int) -> str:
    exponent = code >> 39
    fraction = code & ((1 << 39) - 1)
    return (bytes([exponent]) + fraction.to_bytes(5, "little")).hex()


def main() -> None:
    for increment in range(1, 100):
        source = Fraction((3 << 38) + increment, 1 << 39)
        source_code, source_value = nearest(source)
        middle_code, middle_value = nearest(source_value * Fraction(3, 2))
        final_code, final_value = nearest(middle_value / Fraction(3, 2))
        if final_value != source_value:
            sys.stdout.write(
                f"source {raw(source_code)} middle {raw(middle_code)} "
                f"final {raw(final_code)}\n"
            )
            return
    raise SystemExit("no independently generated round-trip witness")


if __name__ == "__main__":
    main()
