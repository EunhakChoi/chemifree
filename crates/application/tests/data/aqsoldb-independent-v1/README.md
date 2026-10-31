# Independent CSV framing corpus

These 128 synthetic records contain arbitrary text, quotes, commas, empty cells,
UTF-8 and embedded line endings. They contain no AqSolDB source data. Python's
standard-library CSV writer generates `records.csv`; its strict CSV reader checks
the original rows before `expected.json` is emitted. The Rust test compares its
parser output to those independent lexical records. Git preserves the CSV bytes
so CRLF records and embedded line endings remain distinct on every checkout.

Run `python3 generate.py` to verify both files byte for byte, or
`python3 generate.py --write` to regenerate them from fixed synthetic inputs.
