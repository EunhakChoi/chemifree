"""Compare Chemifree's CIP fixtures with RDKit 2022.03.2's new CIP labeler.

Run by hand (it is not part of `npm run check`). The sandbox runner's `--rdkit` option cannot
start the venv interpreter (issue #113: the venv's python is a symlink into ~/.local/share/uv,
which the sandbox's private /home hides), so run the interpreter directly, bounded:

    (ulimit -v 4000000; timeout 120 ~/.cache/chemifree/rdkit-venv/bin/python -I \
        scripts/verify-cip-rdkit.py)

Each fixture in crates/domain/tests/cip-fixtures.json is a SMILES string and, for some atoms
(0-based, in string order), the label Chemifree's domain test
(crates/domain/tests/cip/main.rs) proves it computes: R, S, none, or `unsupported:<reason>`.
The SMILES is read by RDKit alone, so no Chemifree code, and not the test's small SMILES
reader, is involved in this half. For every expected atom the script

1. runs `rdkit.Chem.rdCIPLabeler.AssignCIPLabels` (the new, rule-complete labeler) and reads
   `_CIPCode`; no code means RDKit finds the atom not stereogenic, which is what `none` claims;
2. for R, S and none, requires RDKit to agree, unless the fixture documents `rdkit_differs`;
3. for `unsupported:*`, only reports what RDKit says: those are the cases Chemifree refuses
   (Rules 4 and 5, Rule 3 geometry, aromatic systems it does not read) that an engine with
   more rules labels, so a difference is expected and is not an error;
4. for R and S, also reads the mirror-image SMILES (every `@` swapped with `@@`) and requires
   the opposite descriptor, an RDKit-only check of the mirror-image property.

The fixtures have no double-bond geometry yet (E/Z arrives with the Rule 3 slice), so the
check issue #113 asks for, that a SMILES writer keys double-bond marks by their position in
the string, does not apply: the strings here are written by hand. Agreement on a small corpus
is evidence for the sign and the rule implementation, not equivalence with RDKit or any other
program, and RDKit's labeler is itself a second implementation of the public algorithm. The
public specification's validator was a newer RDKit (2026.03.x); this script pins 2022.03.2.
"""

from __future__ import annotations

import argparse
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


def _rdkit_label(smiles: str, atom: int, what: str) -> str:
    """RDKit's descriptor of one atom: R, S, r, s, or `none` when it assigns no code."""
    mol = rdkit.Chem.MolFromSmiles(smiles)
    if mol is None:
        raise FixtureError(f"RDKit could not read {what}")
    if atom >= mol.GetNumAtoms():
        raise FixtureError(f"{what} has no atom {atom}")
    rdkit.Chem.rdCIPLabeler.AssignCIPLabels(mol)
    found = mol.GetAtomWithIdx(atom)
    return found.GetProp("_CIPCode") if found.HasProp("_CIPCode") else "none"


def _mirror(smiles: str) -> str:
    """The mirror image: every `@@` becomes `@` and every lone `@` becomes `@@`."""
    return smiles.replace("@@", "\0").replace("@", "@@").replace("\0", "@")


def check(fixture: dict[str, Any]) -> tuple[list[str], list[str]]:
    """Returns the result lines and the problems found for one fixture."""
    name = fixture["name"]
    smiles = fixture["smiles"]
    differs = fixture.get("rdkit_differs")
    lines: list[str] = []
    problems: list[str] = []
    for expectation in fixture["expect"]:
        atom, expected = expectation["atom"], expectation["label"]
        seen = _rdkit_label(smiles, atom, f"{name} ({smiles})")
        if expected.startswith("unsupported:"):
            lines.append(
                f"{name} atom {atom}: Chemifree refuses ({expected.split(':', 1)[1]}); "
                f"RDKit says {seen} (informational)"
            )
            continue
        agrees = seen == expected
        if agrees and differs:
            problems.append(f"{name} atom {atom}: RDKit now agrees; drop rdkit_differs")
        if not agrees and not differs:
            problems.append(f"{name} atom {atom}: RDKit says {seen}, fixture says {expected}")
        lines.append(f"{name} atom {atom}: {'agrees' if agrees else 'DIFFERS'} ({expected})")
        if expected in ("R", "S"):
            mirrored = _rdkit_label(_mirror(smiles), atom, f"{name} mirror")
            opposite = "S" if expected == "R" else "R"
            if mirrored != opposite:
                problems.append(
                    f"{name} atom {atom}: mirror image is {mirrored}, expected {opposite}"
                )
    return lines, problems


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
    checked = 0
    for fixture in document["fixtures"]:
        lines, found = check(fixture)
        checked += len(lines)
        for line in lines:
            sys.stdout.write(line + "\n")
        problems.extend(found)
    for problem in problems:
        sys.stderr.write(problem + "\n")
    sys.stdout.write(
        f"RDKit {rdkit.__version__} ({document['rdkit_labeler']}): "
        f"{len(document['fixtures'])} fixtures, {checked} atoms, {len(problems)} problems\n"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
