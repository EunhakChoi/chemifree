"""Tests for the corpus comparison: it must print actual old and new values.

Run with `python -I scripts/test_aromaticity_corpus_diff.py`; pytest also
collects the `test_` functions.
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys
from typing import Any

_PATH = pathlib.Path(__file__).with_name("aromaticity_corpus_diff.py")
_SPEC = importlib.util.spec_from_file_location("corpus_diff", _PATH)
assert _SPEC is not None
assert _SPEC.loader is not None
_MODULE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MODULE)


def _record(identity: dict[str, Any]) -> dict[str, Any]:
    return {
        "name": "x",
        "class": "c",
        "spelling": "kekule",
        "aromaticity": {
            "systems": [],
            "to_aromatic_refused": None,
            "to_kekule_refused": None,
        },
        "identity": identity,
    }


def test_accepted_in_both_with_different_codes_prints_both_codes() -> None:
    changes = _MODULE.describe_changes(
        _record({"code": "aaaa"}), _record({"code": "bbbb"})
    )
    assert changes == [
        ("chemical identity detail", '{"code": "aaaa"} -> {"code": "bbbb"}')
    ]


def test_refused_in_both_with_different_details_prints_both() -> None:
    before = _record({"refused": "r", "detail": "one"})
    after = _record({"refused": "r", "detail": "two"})
    changes = _MODULE.describe_changes(before, after)
    assert len(changes) == 1
    title, detail = changes[0]
    assert title == "chemical identity detail"
    assert '"detail": "one"' in detail
    assert '"detail": "two"' in detail
    assert "ok -> ok" not in detail


def test_changed_refusal_reason_is_reported_once_with_reasons() -> None:
    changes = _MODULE.describe_changes(
        _record({"refused": "a"}), _record({"refused": "b"})
    )
    assert changes == [
        ("chemical identity availability", "refused/a -> refused/b")
    ]


def test_identical_records_have_no_changes() -> None:
    record = _record({"code": "aaaa"})
    assert _MODULE.describe_changes(record, record) == []


def main() -> None:
    """Run every test function in this module."""
    names = [name for name in sorted(globals()) if name.startswith("test_")]
    for name in names:
        globals()[name]()
    sys.stdout.write(f"{len(names)} tests passed\n")


if __name__ == "__main__":
    main()
