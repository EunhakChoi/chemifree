"""Read the stereo corpus as CDXML with an independent small reader and compare with RDKit.

Hand-run (not part of `npm run check`) with the RDKit 2022.03.2 Python:

    python -I scripts/verify-cdxml-rdkit.py

RDKit 2022.03.2 has no CDXML reader (``MolsFromCDXML`` appeared later), so this script
carries its own: it writes each drawing of crates/application/tests/data/stereo-fixtures.json
as CDXML (a fixture mark ``W``/``H`` with the narrow end at the first atom is ``WedgeBegin`` /
``WedgedHashBegin`` on a bond written from that atom, ``W<``/``H<`` is the same picture
written ``...End`` from the other atom, ``w`` is ``Wavy``), parses the text with the
standard-library XML parser, builds a V2000 molblock (y negated, narrow end first) and lets
RDKit assign the stereo. The canonical isomeric SMILES must equal the one written from the
fixture's hand-stated configuration, exactly as scripts/verify-stereo-rdkit.py does for the
molblock route. No Chemifree code is involved, so this checks the CDXML spelling and the
Begin/End rule against RDKit's reading of the picture, not the Rust importer; that is checked
by crates/application/tests/cdxml_stereo_import.rs against the same hand-stated values.

The reading of Begin and End follows the public CDXML fixtures (the narrow end, the
stereocenter, is the bond's B atom for ``...Begin`` and its E atom for ``...End``); RDKit does
not confirm that rule, it only confirms the picture once the rule is applied. Drawings with a
crossed double-bond mark are skipped (the CDXML spelling is not public). Agreement on a small
corpus is not conformance with RDKit or with any other program.
"""

from __future__ import annotations

import importlib.util
import json
import pathlib
import sys
import xml.etree.ElementTree as xml_tree
from typing import Any

import rdkit
import rdkit.Chem

_ROOT = pathlib.Path(__file__).resolve().parent
_DISPLAY = {"W": "WedgeBegin", "H": "WedgedHashBegin", "w": "Wavy"}
_REVERSED = {"W": "WedgeEnd", "H": "WedgedHashEnd"}
_FLAG = {"WedgeBegin": 1, "WedgeEnd": 1, "WedgedHashBegin": 6, "WedgedHashEnd": 6, "Wavy": 4}
_ATOMIC_NUMBER = {"C": 6, "N": 7, "O": 8, "F": 9, "S": 16, "Cl": 17, "Br": 35, "I": 53}


def _load_sibling() -> Any:
    """The fixture script, for its OpenSMILES writer and fixture file (hyphenated name)."""
    spec = importlib.util.spec_from_file_location(
        "verify_stereo_rdkit", _ROOT / "verify-stereo-rdkit.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _cdxml(fixture: dict[str, Any], reverse: bool) -> str | None:
    """The drawing as CDXML; with `reverse` every marked bond is written from its other end with the matching `...End`/`...Begin` name."""
    parts = ['<CDXML><page id="1"><fragment id="2">']
    for index, (symbol, x, y) in enumerate(fixture["atoms"]):
        parts.append(
            f'<n id="{10 + index}" p="{x} {y}" Element="{_ATOMIC_NUMBER[symbol]}"/>'
        )
    for index, (first, second, kind, mark) in enumerate(fixture["bonds"]):
        if mark == "x":
            return None
        order = {"-": 1, "=": 2}.get(kind)
        if order is None:
            return None
        narrow_second = mark.endswith("<")
        base = mark.rstrip("<")
        begin, end, display = first, second, _DISPLAY.get(base, "")
        if narrow_second:
            display = _REVERSED[base] if base in _REVERSED else display
        if reverse and base:
            # Every marked bond is written from its other end with the matching name, so the
            # same picture is stated in the other form.
            begin, end = second, first
            if base in _REVERSED:
                display = _DISPLAY[base] if narrow_second else _REVERSED[base]
        attribute = f' Display="{display}"' if display else ""
        parts.append(
            f'<b id="{100 + index}" B="{10 + begin}" E="{10 + end}" Order="{order}"{attribute}/>'
        )
    parts.append("</fragment></page></CDXML>")
    return "".join(parts)


def _molblock(text: str) -> str:
    """A V2000 molblock from CDXML text: y negated, the narrow end first."""
    fragment = xml_tree.fromstring(text).find("./page/fragment")
    assert fragment is not None
    atoms = fragment.findall("n")
    bonds = fragment.findall("b")
    index = {node.get("id"): number for number, node in enumerate(atoms, start=1)}
    symbols = {value: key for key, value in _ATOMIC_NUMBER.items()}
    lines = ["cdxml", "  verify-cdxml-rdkit", ""]
    lines.append(f"{len(atoms):3d}{len(bonds):3d}  0  0  0  0  0  0  0  0999 V2000")
    for node in atoms:
        x, y = (float(value) for value in node.get("p", "").split())
        symbol = symbols[int(node.get("Element", "6"))]
        lines.append(
            f"{x:10.4f}{-y:10.4f}{0.0:10.4f} {symbol:<3} 0  0  0  0  0  0  0  0  0  0  0  0"
        )
    for bond in bonds:
        begin, end = index[bond.get("B")], index[bond.get("E")]
        display = bond.get("Display", "")
        if display.endswith("End"):
            begin, end = end, begin  # the narrow end is E: write it first
        lines.append(
            f"{begin:3d}{end:3d}{int(bond.get('Order', '1')):3d}{_FLAG.get(display, 0):3d}"
        )
    lines.append("M  END")
    return "\n".join(lines) + "\n"


def main() -> int:
    """Checks every drawable fixture twice (as listed and with reversed bonds)."""
    if rdkit.__version__ != "2022.03.2":
        sys.stderr.write(f"expected RDKit 2022.03.2, found {rdkit.__version__}\n")
        return 2
    sibling = _load_sibling()
    corpus = json.loads(sibling._FIXTURES.read_text(encoding="utf-8"))
    problems = 0
    checked = 0
    skipped = 0
    differing = 0
    for fixture in corpus["fixtures"]:
        name = fixture["name"]
        if "refused" in fixture:
            continue
        expected = sibling._canonical(
            rdkit.Chem.MolFromSmiles(sibling._smiles(fixture)), f"{name} (smiles)"
        )
        first_text = None
        for reverse in (False, True):
            text = _cdxml(fixture, reverse)
            if text is None:
                sys.stdout.write(f"{name}: skipped, a mark or bond kind with no CDXML form here\n")
                skipped += 1
                break
            if reverse and first_text is not None and text != first_text:
                differing += 1
            first_text = text if not reverse else first_text
            seen = sibling._canonical(
                rdkit.Chem.MolFromMolBlock(_molblock(text)), f"{name} (cdxml)"
            )
            agrees = seen == expected
            documented = bool(fixture.get("rdkit_differs"))
            ok = agrees != documented
            checked += 1
            problems += 0 if ok else 1
            sys.stdout.write(
                f"{name}{' (reversed)' if reverse else ''}: "
                f"{'agrees' if agrees else 'DIFFERS'}; stated {expected}; RDKit {seen}"
                f"{'' if ok else '  <-- PROBLEM'}\n"
            )
    sys.stdout.write(f"{differing} reversed readings differ in form from the first pass\n")
    if differing == 0:
        sys.stdout.write("PROBLEM: the reversed pass repeats the first pass\n")
        problems += 1
    sys.stdout.write(f"RDKit {rdkit.__version__}: {checked} readings, {skipped} skipped, {problems} problems\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
