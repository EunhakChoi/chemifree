"""Compare Chemifree's stereo drawing fixtures with RDKit 2022.03.2.

Run by hand (it is not part of `npm run check`), for example through the sandbox
runner with its RDKit option:

    python -I scripts/verify-stereo-rdkit.py

Each fixture in crates/application/tests/data/stereo-fixtures.json is a drawing
with a hand-stated configuration; the Rust test
crates/application/tests/stereo_fixtures.rs proves that Chemifree derives
exactly that from the drawing. This script is the independent half. For every
fixture with a stated configuration it

1. writes its own V2000 molblock from the drawing (y negated to y up, a wedge
   or hash flag on the bond line whose first atom is the narrow end), which
   RDKit reads like any drawn structure and from which it assigns chirality
   and double-bond stereo itself;
2. writes an OpenSMILES string from the stated configuration (the neighbor
   order of each center is rewritten into the order the string lists them), so
   no Chemifree code is involved;
3. compares the two canonical isomeric SMILES.

A fixture whose RDKit reading differs from the stated configuration says why in
its `rdkit_differs` field. The script fails when a fixture without that field
differs or one with it agrees, so a disagreement can be documented but not
hidden. For a refused drawing the fixture says whether RDKit assigns
chirality at all. Agreement on a small corpus is not conformance with RDKit or
with any other program.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any

import rdkit
import rdkit.Chem
import rdkit.RDLogger

rdkit.RDLogger.DisableLog("rdApp.*")
_EXPECTED_RDKIT = "2022.03.2"
_FIXTURES = (
    pathlib.Path(__file__).resolve().parent.parent
    / "crates/application/tests/data/stereo-fixtures.json"
)
_STEREO_FLAG = {"W": 1, "H": 6, "w": 4, "x": 3}
_ORGANIC = {"B", "C", "N", "O", "S", "P", "F", "Cl", "Br", "I"}


class FixtureError(Exception):
    """A fixture the script cannot evaluate."""


def _molblock(fixture: dict[str, Any]) -> str:
    """The drawing as a V2000 molblock, y negated, narrow end first."""
    atoms = fixture["atoms"]
    bonds = fixture["bonds"]
    lines = ["stereo fixture", "  verify-stereo-rdkit", ""]
    lines.append(f"{len(atoms):3d}{len(bonds):3d}  0  0  0  0  0  0  0  0999 V2000")
    for symbol, x, y in atoms:
        lines.append(
            f"{x:10.4f}{-y:10.4f}{0.0:10.4f} {symbol:<3} 0  0  0  0  0  0  0  0  0  0  0  0"
        )
    for first, second, kind, mark in bonds:
        flag = _STEREO_FLAG.get(mark.rstrip("<"), 0)
        if mark.endswith("<"):
            first, second = second, first
        order = {"-": 1, "=": 2}[kind]
        lines.append(f"{first + 1:3d}{second + 1:3d}{order:3d}{flag:3d}")
    lines.append("M  END")
    return "\n".join(lines) + "\n"


def _inversions(sequence: list[int]) -> int:
    return sum(
        1
        for i, earlier in enumerate(sequence)
        for later in sequence[i + 1 :]
        if earlier > later
    )


def _smiles(fixture: dict[str, Any]) -> str:
    """OpenSMILES for the stated configuration, written from the fixture alone."""
    atoms = fixture["atoms"]
    expect = fixture["expect"]
    adjacency: dict[int, list[tuple[int, str]]] = {i: [] for i in range(len(atoms))}
    for first, second, kind, _mark in fixture["bonds"]:
        adjacency[first].append((second, kind))
        adjacency[second].append((first, kind))
    if sum(len(v) for v in adjacency.values()) != 2 * (len(atoms) - 1):
        raise FixtureError(f"{fixture['name']}: the SMILES writer needs a tree")
    centers = {
        c["atom"]: c for c in expect.get("centers", []) if c["turn"] != "unknown"
    }
    # Direction marks next to a double bond: the chain must read ref - A = B - ref.
    directions: dict[tuple[int, int], str] = {}
    for double in expect.get("double_bonds", []):
        if double["relation"] == "unknown":
            continue
        begin, end, _kind, _mark = fixture["bonds"][double["bond"]]
        first_ref, last_ref = double["refs"]
        directions[(first_ref, begin)] = "/"
        directions[(end, last_ref)] = "/" if double["relation"] == "opposite" else "\\"

    def token(atom: int, parent: int | None, children: list[int]) -> str:
        symbol = atoms[atom][0]
        if atom not in centers:
            if symbol not in _ORGANIC:
                raise FixtureError(f"{fixture['name']}: {symbol} needs brackets")
            return symbol
        center = centers[atom]
        order = center["order"]
        listed = ([] if parent is None else [parent])
        extra = [slot for slot in order if isinstance(slot, str)]
        listed += extra + children
        if sorted(map(str, listed)) != sorted(map(str, order)):
            raise FixtureError(f"{fixture['name']}: order of atom {atom} is not its neighbors")
        permutation = [order.index(slot) for slot in listed]
        turn = center["turn"]
        flip = _inversions(permutation) % 2 == 1
        anticlockwise = (turn == "anticlockwise") != flip
        tag = "@" if anticlockwise else "@@"
        hydrogen = "H" if "H" in extra else ""
        return f"[{symbol}{tag}{hydrogen}]"

    def write(atom: int, parent: int | None) -> str:
        children = sorted(n for n, _ in adjacency[atom] if n != parent)
        text = token(atom, parent, children)
        parts = []
        for child in children:
            kind = dict(adjacency[atom])[child]
            bond = "=" if kind == "=" else ""
            direction = directions.get((atom, child), "")
            if (child, atom) in directions:
                raise FixtureError(
                    f"{fixture['name']}: the reference {child} is written after {atom}"
                )
            parts.append(bond + direction + write(child, atom))
        for part in parts[:-1]:
            text += f"({part})"
        return text + (parts[-1] if parts else "")

    # A lone pair stands where an implicit hydrogen would (right after the preceding atom).
    # RDKit's SMILES reader puts it last instead when the center is the first atom of the
    # string (an odd permutation, so the tag would flip) and agrees when the center follows
    # another atom, so the string is rooted at a neighbor of a first-atom lone-pair center.
    # This is a convention of RDKit's SMILES reader, observed with 2022.03.2, not a finding
    # about Chemifree.
    root = 0
    first = centers.get(0)
    if first is not None and "LP" in first["order"]:
        root = min(n for n, _ in adjacency[0])
    return write(root, None)


def _canonical(mol: rdkit.Chem.Mol | None, what: str) -> str:
    if mol is None:
        raise FixtureError(f"RDKit could not read {what}")
    return rdkit.Chem.MolToSmiles(mol)


def check(fixture: dict[str, Any]) -> tuple[str, list[str]]:
    """Returns a one-line result and the problems found for one fixture."""
    name = fixture["name"]
    problems: list[str] = []
    mol = rdkit.Chem.MolFromMolBlock(_molblock(fixture))
    seen = _canonical(mol, f"{name} (molblock)")
    if "refused" in fixture:
        asserted = fixture["refused"]["rdkit_chirality"]
        has_stereo = any(c in seen for c in "@/\\")
        if asserted is not None and asserted != has_stereo:
            problems.append(
                f"{name}: RDKit stereo is {has_stereo} ({seen}), fixture says {asserted}"
            )
        return f"{name}: refused; RDKit reads {seen} (stereo {has_stereo})", problems
    expected = _canonical(rdkit.Chem.MolFromSmiles(_smiles(fixture)), f"{name} (smiles)")
    agrees = seen == expected
    differs = fixture.get("rdkit_differs")
    if agrees and differs:
        problems.append(f"{name}: RDKit now agrees ({seen}); drop rdkit_differs")
    if not agrees and not differs:
        problems.append(f"{name}: RDKit reads {seen}, stated configuration is {expected}")
    verdict = "agrees" if agrees else f"DIFFERS (documented: {differs})"
    return f"{name}: {verdict}; stated {expected}; RDKit {seen}", problems


def main() -> int:
    """Checks every fixture; prints each result and returns the exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--fixtures", type=pathlib.Path, default=_FIXTURES)
    arguments = parser.parse_args()
    if rdkit.__version__ != _EXPECTED_RDKIT:
        sys.stderr.write(f"expected RDKit {_EXPECTED_RDKIT}, found {rdkit.__version__}\n")
        return 2
    document = json.loads(arguments.fixtures.read_text(encoding="utf-8"))
    if document["rdkit_version"] != _EXPECTED_RDKIT:
        sys.stderr.write("the fixture file pins a different RDKit version\n")
        return 2
    problems: list[str] = []
    for fixture in document["fixtures"]:
        line, found = check(fixture)
        sys.stdout.write(line + "\n")
        problems.extend(found)
    for problem in problems:
        sys.stderr.write(problem + "\n")
    sys.stdout.write(
        f"RDKit {rdkit.__version__}: {len(document['fixtures'])} fixtures, "
        f"{len(problems)} problems\n"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
