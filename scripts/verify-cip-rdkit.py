"""Compare Chemifree's CIP fixtures with RDKit 2022.03.2's new CIP labeler.

Run by hand (it is not part of `npm run check`). The sandbox runner's `--rdkit` option cannot
start the venv interpreter (issue #113: the venv's python is a symlink into ~/.local/share/uv,
which the sandbox's private /home hides), so run the interpreter directly, bounded:

    (ulimit -v 4000000; timeout 120 ~/.cache/chemifree/rdkit-venv/bin/python -I \
        scripts/verify-cip-rdkit.py)

Each fixture in crates/domain/tests/cip-fixtures.json is a SMILES string and, for some atoms
(`"atom": n`, 0-based, in string order) and double bonds (`"bond": [i, j]`, the atoms at its
ends), the label Chemifree's domain test (crates/domain/tests/cip/main.rs) proves it computes:
R, S, E, Z, none, `unspecified`, `ambiguous:<reason>` or `unsupported:<reason>`. The SMILES is
read by RDKit alone, so no Chemifree code, and not the test's small SMILES reader, is
involved in this half. For every expectation the script

1. runs `rdkit.Chem.rdCIPLabeler.AssignCIPLabels` (the new, rule-complete labeler) and reads
   `_CIPCode` of the atom or bond; no code means RDKit finds the unit not stereogenic, which is
   what `none` claims (and what an unstored double bond looks like to RDKit);
2. for R, S, E, Z and none, requires RDKit to agree, unless the fixture documents
   `rdkit_differs`;
3. for `unspecified`, `ambiguous:*` and `unsupported:*`, only reports what RDKit says: those
   are the cases Chemifree refuses or cannot decide (Rules 4 and 5, a ligand double bond with
   no stated geometry, aromatic systems it does not read) that an engine with more rules or
   another reading labels, so a difference is expected and is not an error;
4. for R and S, also reads the mirror-image SMILES (every `@` swapped with `@@`) and requires
   the opposite descriptor, an RDKit-only check of the mirror-image property;
5. for every R, S, E, Z and none expectation, writes the same molecule with its atoms
   renumbered in the reverse order (RDKit writes the SMILES, so its `/` and `\\` marks are keyed
   by the order it writes, not by the stored begin and end) and requires the same label on
   the same unit. This is the E/Z order check issue #113 asks for: a label that depended on
   which atom of a double bond is written first would show here.

Which unit a SMILES double-bond mark belongs to is the reader's business, so a fixture whose
marks conflict (two `/` out of one atom) is not used: RDKit would ignore it. Agreement on a
small corpus is evidence for the sign and the rule implementation, not equivalence with RDKit
or any other program, and RDKit's labeler is itself a second implementation of the public
algorithm. The public specification's validator was a newer RDKit (2026.03.x); this script pins
2022.03.2.
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import sys
from typing import Any

import rdkit
import rdkit.Chem
import rdkit.Chem.rdCIPLabeler
import rdkit.RDLogger

rdkit.RDLogger.DisableLog("rdApp.*")
_EXPECTED_RDKIT = "2022.03.2"
_FIXTURES = (
    pathlib.Path(__file__).resolve().parent.parent
    / "crates/domain/tests/cip-fixtures.json"
)


class FixtureError(Exception):
    """A fixture the script cannot evaluate."""


def _unit_label(mol: rdkit.Chem.Mol, unit: Any, what: str) -> str:
    """RDKit's descriptor of an atom (int) or a bond ([i, j]): R, S, r, s, E, Z or `none`."""
    mol = rdkit.Chem.Mol(mol)
    rdkit.Chem.rdCIPLabeler.AssignCIPLabels(mol)
    if isinstance(unit, int):
        if unit >= mol.GetNumAtoms():
            raise FixtureError(f"{what} has no atom {unit}")
        found = mol.GetAtomWithIdx(unit)
    else:
        found = mol.GetBondBetweenAtoms(unit[0], unit[1])
        if found is None:
            raise FixtureError(f"{what} has no bond {unit}")
        if found.GetBondType() != rdkit.Chem.BondType.DOUBLE:
            raise FixtureError(f"{what} bond {unit} is not a double bond")
    return found.GetProp("_CIPCode") if found.HasProp("_CIPCode") else "none"


def _read(smiles: str, what: str) -> rdkit.Chem.Mol:
    mol = rdkit.Chem.MolFromSmiles(smiles)
    if mol is None:
        raise FixtureError(f"RDKit could not read {what}")
    return mol


def _mirror(smiles: str) -> str:
    """The mirror image: every `@@` becomes `@` and every lone `@` becomes `@@`."""
    return smiles.replace("@@", "\0").replace("@", "@@").replace("\0", "@")


def _reversed(mol: rdkit.Chem.Mol) -> tuple[str, list[int]]:
    """The molecule written with its atoms renumbered in the reverse order, and for each atom
    of the original the index it has in the written SMILES."""
    count = mol.GetNumAtoms()
    renumbered = rdkit.Chem.RenumberAtoms(mol, list(range(count))[::-1])
    smiles = rdkit.Chem.MolToSmiles(renumbered, canonical=False)
    written = renumbered.GetProp("_smilesAtomOutputOrder").strip("[],").split(",")
    position = {int(index): at for at, index in enumerate(written)}
    # `renumbered` atom k is original atom count - 1 - k.
    return smiles, [position[count - 1 - old] for old in range(count)]


def check(fixture: dict[str, Any]) -> tuple[list[str], list[str], collections.Counter[str]]:
    """Returns the result lines, the problems found and the counts for one fixture.

    The counts are `atoms` and `bonds` (expectations compared with RDKit: R, S, E, Z, none) and
    `informational` (unsupported, ambiguous or unspecified rows, which are only reported).
    """
    name = fixture["name"]
    smiles = fixture["smiles"]
    differs = fixture.get("rdkit_differs")
    mol = _read(smiles, f"{name} ({smiles})")
    reversed_smiles, where = _reversed(mol)
    reversed_mol = _read(reversed_smiles, f"{name} reversed ({reversed_smiles})")
    lines: list[str] = []
    problems: list[str] = []
    counts: collections.Counter[str] = collections.Counter()
    for expectation in fixture["expect"]:
        unit = expectation["atom"] if "atom" in expectation else expectation["bond"]
        expected = expectation["label"]
        kind = "atom" if "atom" in expectation else "bond"
        shown = f"{kind} {unit}"
        seen = _unit_label(mol, unit, f"{name} ({smiles})")
        if expected.split(":", 1)[0] in ("unsupported", "ambiguous", "unspecified"):
            counts["informational"] += 1
            lines.append(
                f"{name} {shown}: Chemifree says {expected}; RDKit says {seen} (informational)"
            )
            continue
        counts[f"{kind}s"] += 1
        agrees = seen == expected
        if agrees and differs:
            problems.append(f"{name} {shown}: RDKit now agrees; drop rdkit_differs")
        if not agrees and not differs:
            problems.append(f"{name} {shown}: RDKit says {seen}, fixture says {expected}")
        lines.append(f"{name} {shown}: {'agrees' if agrees else 'DIFFERS'} ({expected})")
        if not agrees:
            continue
        moved = where[unit] if isinstance(unit, int) else [where[unit[0]], where[unit[1]]]
        again = _unit_label(reversed_mol, moved, f"{name} reversed")
        if again != expected:
            problems.append(
                f"{name} {shown}: with the atoms in reverse order RDKit says {again}, "
                f"expected {expected} ({reversed_smiles})"
            )
        if expected in ("R", "S"):
            mirrored = _unit_label(
                _read(_mirror(smiles), f"{name} mirror"), unit, f"{name} mirror"
            )
            opposite = "S" if expected == "R" else "R"
            if mirrored != opposite:
                problems.append(
                    f"{name} {shown}: mirror image is {mirrored}, expected {opposite}"
                )
    return lines, problems, counts


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
    totals: collections.Counter[str] = collections.Counter()
    for fixture in document["fixtures"]:
        lines, found, counts = check(fixture)
        totals.update(counts)
        for line in lines:
            sys.stdout.write(line + "\n")
        problems.extend(found)
    for problem in problems:
        sys.stderr.write(problem + "\n")
    sys.stdout.write(
        f"RDKit {rdkit.__version__} ({document['rdkit_labeler']}): "
        f"{len(document['fixtures'])} fixtures, {totals['atoms']} atoms and "
        f"{totals['bonds']} bonds compared, {totals['informational']} informational rows "
        f"(not compared), {len(problems)} problems\n"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
