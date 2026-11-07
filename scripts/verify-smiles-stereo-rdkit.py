"""Compare SMILES stereo reading and recorded writing with an independent RDKit.

Covers tetrahedral marks and CXSMILES groups (P29c) and the directional bonds `/` and `\\`
read as double-bond configurations (P29c2a).

Hand-run, like verify-v3000-rdkit.py; not part of `npm run check`. Build the CLI and pass its
absolute path:

    python -I scripts/verify-smiles-stereo-rdkit.py /path/to/chemifree

For every fixture RDKit parses the text itself and labels each tetrahedral center with its
CIP code (`rdCIPLabeler`, which shares no code with Chemifree). Chemifree imports the same text
through `document.import` and `chem.cip` labels the configurations it stored. Atom k of the
text is RDKit atom k and Chemifree atom `a(k+1)`, so labels are compared per atom, never as
sets; an atom RDKit labels and Chemifree does not, or the reverse, fails. Enhanced stereo
groups of a CXSMILES text are compared as (kind, member atoms) partitions, never by number,
because RDKit renumbers. RDKit also parses recorded exports; their atom order is
mapped back to the imported input before comparing labels and groups.

Refused fixtures state the code Chemifree must refuse with; the script also records what RDKit
makes of the text, which is not an oracle for a refusal. The RDKit used is printed: the groups
check needs a release that reads the `a:`, `o<n>:` and `&<n>:` fields.

Double bonds (P29c2a). For an accepted fixture RDKit embeds the molecule in 3D (ETKDG, fixed
seed) and the script measures the dihedral of the two neighbours Chemifree stored for each
double bond: under 90 degrees is the same side, otherwise opposite sides. That geometry shares
nothing with Chemifree's reading of `/` and `\\`, and is compared with the stored relation. The
double bonds RDKit labels with `rdCIPLabeler` (E or Z) must be exactly those `chem.cip` labels
on the stored configurations, with the same letter. A fixture lists how many double bonds
Chemifree must store; a one-sided double bond served by its neighbour (`C/C=C/C=CC`) stores
none and RDKit must not label it either. Embedding tries a fixed list of seeds; a stored double
bond that no seed embeds is a FAILURE, never a note, so the summary line counts every geometry
comparison made and none can be skipped quietly. Exports of a fixture with stored
double bonds must be refused as `loss_consent_required` for `stereo_configuration`: the writers
write no `/` or `\\` yet. Refusal fixtures also print what RDKit makes of the text (it reads
and ignores many of them), which is information, not an oracle.

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
import rdkit.Chem.AllChem
import rdkit.Chem.rdCIPLabeler
import rdkit.Chem.rdMolTransforms
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
    # How many double-bond configurations Chemifree must store.
    double_bonds: int = 0


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
    Fixture("F/C=C/F", double_bonds=1),
    Fixture("F\\C=C\\F", double_bonds=1),
    Fixture("F\\C=C/F", double_bonds=1),
    Fixture("F/C=C\\F", double_bonds=1),
    Fixture("C(\\F)=C/F", double_bonds=1),
    Fixture("C(/F)=C/F", double_bonds=1),
    Fixture("F/C(CC)=C/F", double_bonds=1),
    # The two neighbours of an end lie on opposite sides: `\CC` would put both below.
    Fixture("F/C(/CC)=C/F", double_bonds=1),
    Fixture("C/C=C/C=C/C", double_bonds=2),
    Fixture("C/C=C\\C=C/C", double_bonds=2),
    Fixture("C/C=C/C=CC", double_bonds=1),  # the second double bond states nothing
    Fixture("F/C=C/C/C=C\\C", double_bonds=2),
    Fixture("F/C=C/CC=CC", double_bonds=1),
    Fixture("[H]/C(F)=C/F", double_bonds=1),
    Fixture("F/[CH]=[CH]/F", double_bonds=1),
    Fixture("C/C=N/O", double_bonds=1),
    Fixture("C/C(C)=N\\O", double_bonds=1),
    Fixture("F/N=N/F", double_bonds=1),
    Fixture("F/N=N\\F", double_bonds=1),
    Fixture("OC(=O)/C=C/C(=O)O", double_bonds=1),
    Fixture("OC(=O)/C=C\\C(=O)O", double_bonds=1),
    Fixture("c1ccccc1/C=C/c1ccccc1", double_bonds=1),
    Fixture("C/C=C/[C@H](F)Cl", double_bonds=1),
    Fixture("C1CC/C=C\\CCC1", double_bonds=1),  # eight atoms: the smallest ring read
    Fixture("C1CCC/C=C/CCC1", double_bonds=1),
    Fixture("C/C=C/C.C/C=C\\C", double_bonds=2),
    Fixture("CC(C)/C=C/C(C)C", double_bonds=1),
    Fixture("C(C(\\F)=C/F)C", double_bonds=1),
    Fixture("C(C)(/F)=C/F", double_bonds=1),
    Fixture("C1=CC=CC=C1/C=C/F", double_bonds=1),
    Fixture("Cl/C=C\\F", double_bonds=1),
    Fixture("C/C=C", refused="directional_bond_incomplete"),
    Fixture("C/C=CC", refused="directional_bond_incomplete"),
    Fixture("F/CC", refused="directional_bond_without_double_bond"),
    Fixture("C/C/C", refused="directional_bond_without_double_bond"),
    Fixture("C/C(\\F)=C/C", refused="directional_bond_conflict"),
    Fixture("F/C(\\CC)=C/F", refused="directional_bond_conflict"),
    Fixture("F/C=C=C=C/F", refused="directional_bond_cumulated"),
    Fixture("F/C=C=C/F", refused="directional_bond_cumulated"),
    Fixture("C1C/C=C\\CCC1", refused="directional_bond_small_ring"),
    Fixture("C/1=C/C=C\\C=C/C=C\\1", refused="directional_bond_unsupported"),
    Fixture("c1ccc/cc1", refused="directional_bond_unsupported"),
    Fixture("F/P=C/F", refused="directional_bond_end_unsupported"),
    Fixture("Cl/[CH](F)=C/F", refused="directional_bond_end_unsupported"),
    Fixture("[CH2](/F)=C/F", refused="directional_bond_end_unsupported"),
    Fixture("C1=CC=CC=C1/C", refused="directional_bond_without_double_bond"),
    Fixture("c1ccccc1/C", refused="directional_bond_without_double_bond"),
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
    written = session.ask(
        "document.export",
        {"document_id": document, "format": "smiles", "style": "recorded"},
    )
    return {
        "labels": labels,
        "groups": sorted(groups),
        "stored": stored,
        "written": written,
        "double_bonds": _stored_double_bonds(snapshot, cip),
    }


def _index(atom_id: str) -> int:
    """Chemifree atom `a(k+1)` is atom k of the text."""
    return int(atom_id[1:]) - 1


def _stored_double_bonds(
    snapshot: dict[str, Any], cip: dict[str, Any]
) -> dict[tuple[int, int], dict[str, Any]]:
    """The stored double-bond configurations by sorted atom pair, with the CIP label."""
    document = snapshot["result"]["document"]
    ends = {
        bond["id"]: (_index(bond["begin"]), _index(bond["end"]))
        for bond in document["bonds"]
    }
    labels = {
        item["bond"]: item["descriptor"]
        for item in cip["result"]["bonds"]
        if item["state"] == "assigned"
    }
    found: dict[tuple[int, int], dict[str, Any]] = {}
    for item in document["stereo"]["bonds"]:
        begin, end = ends[item["bond"]]
        configuration = item["configuration"]
        found[(min(begin, end), max(begin, end))] = {
            "begin": begin,
            "end": end,
            "begin_neighbor": _index(configuration["begin_neighbor"]),
            "end_neighbor": _index(configuration["end_neighbor"]),
            "same_side": configuration["relation"] == "same_side",
            "cip": labels.get(item["bond"]),
        }
    return found


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
    bond_labels = {
        (
            min(bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()),
            max(bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()),
        ): bond.GetProp("_CIPCode")
        for bond in mol.GetBonds()
        if bond.HasProp("_CIPCode")
    }
    return {
        "parsed": True,
        "labels": labels,
        "groups": groups,
        "bond_labels": bond_labels,
        "mol": mol,
    }


# The same ETKDG policy under every seed; the first that embeds is used.
_EMBED_SEEDS = (0xF00D, 1, 2, 3, 4, 5, 6, 7, 8, 9)


@dataclass
class Tally:
    """Geometry comparisons made, so the summary can say how many there were, and the
    information (not oracle) notes printed about refused texts."""

    compared: int = 0
    notes: list[str] = field(default_factory=list)


def _rdkit_same_side(
    mol: rdkit.Chem.Mol, atoms: tuple[int, int, int, int]
) -> tuple[bool | None, str]:
    """Whether the end atoms of the 4-atom path lie on one side of its middle bond.

    Embeds `mol` in 3D with ETKDG, trying the seeds of `_EMBED_SEEDS` in order, and measures the
    dihedral. Returns `(None, reason)` when no seed embeds the molecule.
    """
    embeddable = rdkit.Chem.AddHs(mol)
    for seed in _EMBED_SEEDS:
        parameters = rdkit.Chem.AllChem.ETKDGv3()
        parameters.randomSeed = seed
        parameters.maxIterations = 200
        if rdkit.Chem.AllChem.EmbedMolecule(embeddable, parameters) == 0:
            break
    else:
        return None, f"RDKit ETKDG could not embed the molecule with {len(_EMBED_SEEDS)} seeds"
    angle = rdkit.Chem.rdMolTransforms.GetDihedralDeg(
        embeddable.GetConformer(), *atoms
    )
    return abs(angle) < 90.0, f"dihedral {angle:.1f}"


def _describe_rdkit(text: str) -> str:
    """What RDKit makes of a text Chemifree refuses: information, not an oracle."""
    theirs = _rdkit(text)
    if not theirs["parsed"]:
        return "RDKit does not parse it"
    if theirs["bond_labels"]:
        return f"RDKit parses it and labels double bonds {theirs['bond_labels']}"
    return "RDKit parses it and states no double-bond stereo (ignores the marks)"


def _check_double_bonds(
    fixture: Fixture,
    ours: dict[str, Any],
    theirs: dict[str, Any],
    tally: Tally,
) -> list[str]:
    """Compares the stored double bonds with RDKit's geometry and CIP labels."""
    failures: list[str] = []
    stored = ours["double_bonds"]
    if len(stored) != fixture.double_bonds:
        failures.append(
            f"{fixture.text}: stored {len(stored)} double bonds, expected {fixture.double_bonds}"
        )
    ours_cip = {pair: info["cip"] for pair, info in stored.items() if info["cip"] is not None}
    if ours_cip != theirs["bond_labels"]:
        failures.append(
            f"{fixture.text}: double-bond CIP labels differ: "
            f"Chemifree {ours_cip}, RDKit {theirs['bond_labels']}"
        )
    for pair, info in stored.items():
        path = (info["begin_neighbor"], info["begin"], info["end"], info["end_neighbor"])
        same_side, detail = _rdkit_same_side(theirs["mol"], path)
        if same_side is None:
            failures.append(f"{fixture.text}: bond {pair} not compared: {detail}")
            continue
        tally.compared += 1
        if same_side != info["same_side"]:
            failures.append(
                f"{fixture.text}: bond {pair} stored same_side={info['same_side']}, "
                f"RDKit geometry same_side={same_side} ({detail})"
            )
        else:
            _LOGGER.info("%s: bond %s same_side=%s (%s)", fixture.text, pair, same_side, detail)
    return failures


def _check_consent(fixture: Fixture, written: dict[str, Any]) -> list[str]:
    """A recorded export of a stored double bond is refused until the loss is accepted."""
    diagnostics = written.get("diagnostics") or [{}]
    first = diagnostics[0]
    if (
        written["outcome"] != "error"
        or first.get("code") != "loss_consent_required"
        or first["details"]["features"] != ["stereo_configuration"]
    ):
        return [f"{fixture.text}: recorded export should need stereo_configuration consent: {written}"]
    return []


def _check(session: Session, tally: Tally) -> list[str]:
    failures: list[str] = []
    for fixture in FIXTURES:
        ours = _chemifree(session, fixture.text)
        if fixture.refused is not None:
            if ours.get("refused") != fixture.refused:
                failures.append(f"{fixture.text}: expected refusal {fixture.refused}, got {ours}")
            else:
                tally.notes.append(f"refused {fixture.text} ({fixture.refused}): {_describe_rdkit(fixture.text)}")
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
                f"{fixture.text}: CIP labels differ: "
                f"Chemifree {ours['labels']}, "
                f"RDKit {theirs['labels']}"
            )
        if ours["groups"] != sorted(fixture.groups):
            failures.append(
                f"{fixture.text}: groups {ours['groups']}, "
                f"expected {fixture.groups}"
            )
        if theirs["groups"] != sorted(fixture.groups):
            failures.append(
                f"{fixture.text}: RDKit groups {theirs['groups']}, "
                f"expected {fixture.groups}"
            )
        failures.extend(_check_double_bonds(fixture, ours, theirs, tally))
        written = ours["written"]
        if ours["double_bonds"]:
            failures.extend(_check_consent(fixture, written))
            continue
        if written["outcome"] not in ("ok", "partial"):
            failures.append(
                f"{fixture.text}: recorded export refused: {written}"
            )
            continue
        result = written["result"]
        reread = _rdkit(result["content"])
        if not reread["parsed"]:
            failures.append(
                f"{fixture.text}: RDKit refused recorded output "
                f"{result['content']!r}"
            )
            continue
        order = [int(atom[1:]) - 1 for atom in result["atom_order"]]
        labels = {
            order[atom]: label for atom, label in reread["labels"].items()
        }
        groups = sorted(
            (kind, tuple(sorted(order[atom] for atom in members)))
            for kind, members in reread["groups"]
        )
        if labels != theirs["labels"] or groups != theirs["groups"]:
            failures.append(
                f"{fixture.text}: RDKit read recorded output "
                f"{result['content']!r} as "
                f"labels {labels}, groups {groups}; expected {theirs}"
            )
        _LOGGER.info(
            "%s: %s groups=%s", fixture.text, ours["labels"], ours["groups"]
        )
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
        tally = Tally()
        try:
            failures = _check(session, tally)
        finally:
            session.close()
    for note in tally.notes:
        _LOGGER.info("NOTE %s", note)
    for failure in failures:
        _LOGGER.error("FAIL %s", failure)
    _LOGGER.info(
        "%d fixtures, %d double-bond geometry comparisons, %d failures",
        len(FIXTURES) + len(RESPELLINGS),
        tally.compared,
        len(failures),
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
