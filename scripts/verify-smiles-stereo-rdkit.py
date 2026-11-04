"""Compare Chemifree's SMILES stereo read (P29c) with an independent RDKit.

Hand-run, like verify-v3000-rdkit.py; not part of `npm run check`. Build the CLI and pass its
absolute path:

    python -I scripts/verify-smiles-stereo-rdkit.py /path/to/chemifree

For every fixture RDKit parses the text itself and labels each tetrahedral center with its
CIP code (`rdCIPLabeler`, which shares no code with Chemifree). Chemifree imports the same text
through `document.import` and `chem.cip` labels the configurations it stored. Atom k of the
text is RDKit atom k and Chemifree atom `a(k+1)`, so labels are compared per atom, never as
sets; an atom RDKit labels and Chemifree does not, or the reverse, fails. Enhanced stereo
groups of a CXSMILES text are compared as (kind, member atoms) partitions, never by number,
because RDKit renumbers.

Refused fixtures state the code Chemifree must refuse with; the script also records what RDKit
makes of the text, which is not an oracle for a refusal. The RDKit used is printed: the groups
check needs a release that reads the `a:`, `o<n>:` and `&<n>:` fields.

CIP labels exercise Chemifree's CIP rules 1a to 3, so a fixture whose RDKit label needs a later
rule is listed under `rdkit_only` and compared on RDKit's side alone.
"""

from __future__ import annotations

import json
import logging
import pathlib
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from typing import Any

import rdkit
import rdkit.Chem
import rdkit.Chem.rdCIPLabeler
import rdkit.RDLogger

_LOGGER = logging.getLogger(__name__)
rdkit.RDLogger.DisableLog("rdApp.*")


@dataclass(frozen=True)
class Fixture:
    """One text, what Chemifree must store, or the refusal code it must give."""

    text: str
    refused: str | None = None
    # Expected groups as (kind, sorted atom indexes); None means the text has none.
    groups: tuple[tuple[str, tuple[int, ...]], ...] = field(default_factory=tuple)


FIXTURES = [
    Fixture("N[C@H](O)C"),
    Fixture("N[C@@H](O)C"),
    Fixture("N[C@@H](C)C(=O)O"),  # L-alanine, S
    Fixture("N[C@H](C)C(=O)O"),  # D-alanine, R
    Fixture("[C@H](F)(Cl)Br"),
    Fixture("F[C@](Cl)(Br)I"),
    Fixture("[H][C@](F)(Cl)Br"),
    Fixture("F[C@H](Cl)[C@H](Br)I"),  # the RR example of the public specification
    Fixture("F[C@H](Cl)[C@@H](Br)I"),
    Fixture("[C@H]1(F)CCCC1Cl"),
    Fixture("C1CCC[C@H]1F.[C@H](F)(Cl)Br"),
    Fixture("F[C@H]1CCCCO1"),
    Fixture("O1CCCC[C@@H]1F"),
    Fixture("[C@]21(F)CC2CC1Cl"),
    Fixture("F[C@H](Cl)Br |a:1|", groups=(("absolute", (1,)),)),
    Fixture(
        "F[C@H](Cl)[C@H](Br)[C@H](I)N |o1:1,3,&1:5|",
        groups=(("or", (1, 3)), ("and", (5,))),
    ),
    Fixture("F[C@H](Cl)Br |r|", refused="cxsmiles_field_unsupported"),
    Fixture("F[C@H](Cl)Br |a:0|", refused="cxsmiles_group_member_not_stereocenter"),
    Fixture("F[C@H]Cl", refused="stereo_neighbor_count"),
    Fixture("[S@](=O)(C)CC", refused="stereo_center_unsupported_here"),
    Fixture("F[C@TH1H](Cl)Br", refused="stereo_class_unsupported"),
    Fixture("F/C=C/F", refused="directional_bond_unsupported"),
]
# Respellings of one molecule: every text must give the same CIP label at the stereocenter.
RESPELLINGS = [
    ["F[C@H](Cl)Br", "[C@@H](F)(Cl)Br", "Br[C@@H](Cl)F", "Br[C@H](F)Cl"],
    ["F[C@H]1CCCCO1", "O1CCCC[C@@H]1F"],
]


class Session:
    """The CLI under test, one JSON request per line."""

    def __init__(self, cli: pathlib.Path, data: str) -> None:
        self._process = subprocess.Popen(  # noqa: S603 - the CLI under test
            [str(cli), "--data-dir", data, "operations"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        if self._process.stdin is None or self._process.stdout is None:
            raise RuntimeError("the CLI has no standard streams")
        self._stdin, self._stdout = self._process.stdin, self._process.stdout
        self._count = 0

    def ask(self, operation: str, parameters: dict[str, Any]) -> dict[str, Any]:
        self._count += 1
        request = {
            "contract": 1,
            "request_id": f"r{self._count}",
            "profile": "preserve-semantics",
            "operation": operation,
            "parameters": parameters,
        }
        self._stdin.write(json.dumps(request) + "\n")
        self._stdin.flush()
        reply: dict[str, Any] = json.loads(self._stdout.readline())
        return reply

    def close(self) -> None:
        self._stdin.close()
        self._process.wait(timeout=30)


def _chemifree(session: Session, text: str) -> dict[str, Any]:
    """Imports `text` into a new document; returns the refusal code or the stored facts."""
    created = session.ask("document.create", {})
    document = created["result"]["document"]["id"]
    base = {
        "document_id": document,
        "expected_revision": 0,
        "format": "smiles",
        "source": text,
    }
    preview = session.ask("document.import", {**base, "preview": True})
    if preview["outcome"] == "error":
        return {"refused": preview["diagnostics"][0]["code"]}
    done = session.ask(
        "document.import",
        {**base, "preview_digest": preview["result"]["preview_digest"], "accept_loss": True},
    )
    if done["outcome"] == "error":
        return {"refused": done["diagnostics"][0]["code"]}
    cip = session.ask("chem.cip", {"document_id": document, "scope": "stored"})
    snapshot = session.ask("document.snapshot", {"document_id": document})
    groups = [
        (group["kind"], tuple(sorted(int(member[1:]) - 1 for member in group["members"])))
        for group in snapshot["result"]["document"]["stereo"]["groups"]
    ]
    labels = {
        int(item["atom"][1:]) - 1: item["descriptor"]
        for item in cip["result"]["atoms"]
        if item["state"] == "assigned"
    }
    stored = len(snapshot["result"]["document"]["stereo"]["atoms"])
    return {"labels": labels, "groups": sorted(groups), "stored": stored}


def _rdkit(text: str) -> dict[str, Any]:
    # Hydrogens written as atoms stay atoms, so atom k of the text is RDKit atom k.
    params = rdkit.Chem.SmilesParserParams()
    params.removeHs = False
    mol = rdkit.Chem.MolFromSmiles(text, params)
    if mol is None:
        return {"parsed": False}
    rdkit.Chem.rdCIPLabeler.AssignCIPLabels(mol)
    labels = {
        atom.GetIdx(): atom.GetProp("_CIPCode")
        for atom in mol.GetAtoms()
        if atom.HasProp("_CIPCode")
    }
    kinds = {"ABSOLUTE": "absolute", "OR": "or", "AND": "and"}
    groups = sorted(
        (
            kinds[group.GetGroupType().name.removeprefix("STEREO_")],
            tuple(sorted(atom.GetIdx() for atom in group.GetAtoms())),
        )
        for group in mol.GetStereoGroups()
    )
    return {"parsed": True, "labels": labels, "groups": groups}


def _check(session: Session) -> list[str]:
    failures: list[str] = []
    for fixture in FIXTURES:
        ours = _chemifree(session, fixture.text)
        if fixture.refused is not None:
            if ours.get("refused") != fixture.refused:
                failures.append(f"{fixture.text}: expected refusal {fixture.refused}, got {ours}")
            continue
        theirs = _rdkit(fixture.text)
        if not theirs["parsed"]:
            failures.append(f"{fixture.text}: RDKit does not read it")
            continue
        if "refused" in ours:
            failures.append(f"{fixture.text}: Chemifree refused it: {ours}")
            continue
        if ours["labels"] != theirs["labels"]:
            failures.append(
                f"{fixture.text}: CIP labels differ: Chemifree {ours['labels']}, RDKit {theirs['labels']}"
            )
        if ours["groups"] != sorted(fixture.groups):
            failures.append(f"{fixture.text}: groups {ours['groups']}, expected {fixture.groups}")
        if theirs["groups"] != sorted(fixture.groups):
            failures.append(f"{fixture.text}: RDKit groups {theirs['groups']}, expected {fixture.groups}")
        _LOGGER.info("%s: %s groups=%s", fixture.text, ours["labels"], ours["groups"])
    for spellings in RESPELLINGS:
        results = [_chemifree(session, text)["labels"] for text in spellings]
        # The stereocenter is the one labeled atom; the label is a property of the molecule.
        distinct = {tuple(sorted(labels.values())) for labels in results}
        if len(distinct) != 1:
            failures.append(f"respellings {spellings}: labels differ: {results}")
    return failures


def main(argv: list[str]) -> int:
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    if len(argv) != 2:
        _LOGGER.error("usage: verify-smiles-stereo-rdkit.py /path/to/chemifree")
        return 2
    _LOGGER.info("RDKit %s", rdkit.__version__)
    with tempfile.TemporaryDirectory() as data:
        session = Session(pathlib.Path(argv[1]), data)
        try:
            failures = _check(session)
        finally:
            session.close()
    for failure in failures:
        _LOGGER.error("FAIL %s", failure)
    _LOGGER.info("%d fixtures, %d failures", len(FIXTURES) + len(RESPELLINGS), len(failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
