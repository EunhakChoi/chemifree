"""Compare two runs of the `aromaticity_corpus` example as Markdown.

    python -I scripts/aromaticity_corpus_diff.py old.jsonl new.jsonl

Records are matched by (name, spelling). Output groups every changed record by
what changed and lists the counts per corpus class.
"""

from __future__ import annotations

import collections
from collections.abc import Callable
import json
import pathlib
import sys
from typing import Any

_Record = dict[str, Any]


def _load(path: str) -> dict[tuple[str, str], _Record]:
    records: dict[tuple[str, str], _Record] = {}
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        record = json.loads(line)
        records[(record["name"], record["spelling"])] = record
    return records


def _systems(record: _Record) -> str:
    systems = record["aromaticity"]["systems"]
    if not systems:
        return "no ring"
    return "; ".join(
        f"{s['classification']}"
        + (f"/{s['reason']}" if s["reason"] else "")
        + f" ({s['atoms']} atoms)"
        for s in systems
    )


def _identity(record: _Record) -> str:
    identity = record["identity"]
    return f"refused/{identity['refused']}" if "refused" in identity else "ok"


def _conversion(record: _Record) -> str:
    aromatic = record["aromaticity"]["to_aromatic_refused"] or "ok"
    kekule = record["aromaticity"]["to_kekule_refused"] or "ok"
    return f"aromatic: {aromatic}, kekule: {kekule}"


_KINDS: list[tuple[str, Callable[[_Record], str]]] = [
    ("classification or reason", _systems),
    ("chemical identity availability", _identity),
    ("conversion outcome", _conversion),
]


def main(argv: list[str]) -> None:
    """Print the comparison of the two JSON Lines files named as arguments."""
    if len(argv) != 3:
        raise SystemExit(f"usage: {argv[0]} <old.jsonl> <new.jsonl>")
    old, new = _load(argv[1]), _load(argv[2])
    if old.keys() != new.keys():
        raise SystemExit("the two runs cover different records")
    groups: dict[str, list[str]] = collections.defaultdict(list)
    per_class: collections.Counter[str] = collections.Counter()
    totals: collections.Counter[str] = collections.Counter(
        record["class"] for record in new.values()
    )
    changed_records = 0
    for key in sorted(new):
        before, after = old[key], new[key]
        changes: list[tuple[str, str]] = []
        for title, describe in _KINDS:
            if describe(before) != describe(after):
                changes.append(
                    (title, f"{describe(before)} -> {describe(after)}")
                )
        if _identity(before) == _identity(after) and (
            before["identity"] != after["identity"]
        ):
            changes.append(
                ("chemical identity code", "ok -> ok (code differs)")
            )
        if not changes:
            continue
        changed_records += 1
        per_class[after["class"]] += 1
        for title, detail in changes:
            groups[title].append(
                f"| {after['class']} | {key[0]} | {key[1]} | {detail} |"
            )
    out = sys.stdout
    unchanged = len(new) - changed_records
    out.write(
        f"Records: {len(new)}; changed: {changed_records}; "
        f"unchanged: {unchanged}\n\n"
    )
    out.write("| class | records | changed |\n|---|---|---|\n")
    for name in sorted(totals):
        out.write(f"| {name} | {totals[name]} | {per_class[name]} |\n")
    for title, rows in sorted(groups.items()):
        out.write(f"\n### {title} ({len(rows)})\n\n")
        out.write(
            "| class | molecule | spelling | change |\n|---|---|---|---|\n"
        )
        out.write("\n".join(rows) + "\n")


if __name__ == "__main__":
    main(sys.argv)
