"""Generate or verify an independent CSV parser corpus without external data."""

import argparse
import csv
import hashlib
import io
import json
from pathlib import Path


def corpus() -> tuple[bytes, bytes]:
    """Return reproducible CSV and its separately checked lexical records."""
    alphabet = [
        "alpha",
        "",
        " leading ",
        "comma,value",
        'quote"value',
        "line\nbreak",
        "windows\r\nbreak",
        "é漢",
        "0007",
        "-0.000",
    ]
    rows = []
    for index in range(128):
        digest = hashlib.sha256(f"independent-csv-{index}".encode()).digest()
        count = 1 + digest[0] % 9
        rows.append(
            [alphabet[value % len(alphabet)] for value in digest[1 : count + 1]]
        )
    if {cell for row in rows for cell in row} != set(alphabet):
        raise ValueError("CSV corpus does not cover all synthetic cell shapes")
    if {len(row) for row in rows} != set(range(1, 10)):
        raise ValueError("CSV corpus does not cover all synthetic row widths")
    stream = io.StringIO(newline="")
    csv.writer(stream, lineterminator="\r\n").writerows(rows)
    text = stream.getvalue()
    if list(csv.reader(io.StringIO(text, newline=""), strict=True)) != rows:
        raise ValueError("CSV reader did not reproduce the generated rows")
    expected = json.dumps(rows, ensure_ascii=False, separators=(",", ":"))
    return text.encode("utf-8"), expected.encode("utf-8")


def main() -> None:
    """Verify committed fixtures, or replace them when explicitly requested."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write", action="store_true", help="write the generated fixtures"
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    for name, contents in zip(
        ("records.csv", "expected.json"), corpus(), strict=True
    ):
        path = root / name
        if args.write:
            path.write_bytes(contents)
        elif path.read_bytes() != contents:
            raise SystemExit(f"fixture differs: {name}")


if __name__ == "__main__":
    main()
