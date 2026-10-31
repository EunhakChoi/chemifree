"""Compare the public MOL operations with an independent RDKit reader/writer."""

# ruff: noqa: T201 - A command-line check reports its observed comparisons.

from __future__ import annotations

import io
import json
import os
import pathlib
import subprocess
import tempfile
from typing import Any

from rdkit import Chem
from rdkit import rdBase
from rdkit.Chem import AllChem

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = pathlib.Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target")))
BINARY = TARGET / "debug/chemifree"


def _request(
    child: subprocess.Popen[str],
    request_id: str,
    operation: str,
    parameters: dict[str, Any],
    *,
    allow_error: bool = False,
) -> dict[str, Any]:
    """Send one operation to the long-lived CLI and read its reply."""
    if child.stdin is None or child.stdout is None:
        raise RuntimeError("The CLI pipes were not opened")
    payload = {
        "contract": 1,
        "request_id": request_id,
        "profile": "preserve-semantics",
        "operation": operation,
        "parameters": parameters,
    }
    child.stdin.write(json.dumps(payload) + "\n")
    child.stdin.flush()
    reply: dict[str, Any] = json.loads(child.stdout.readline())
    if reply["outcome"] not in ("ok", "partial") and not allow_error:
        raise RuntimeError(f"{request_id}: {reply}")
    return reply


def _decode(text: str) -> Any:
    """Use RDKit strict parsing without chemical sanitization."""
    molecule = Chem.MolFromMolBlock(
        text, sanitize=False, removeHs=False, strictParsing=True
    )
    if molecule is None:
        raise RuntimeError("RDKit rejected a MOL connection table")
    return molecule


def _describe(
    molecule: Any,
) -> tuple[list[tuple[Any, ...]], list[tuple[Any, ...]]]:
    """Get atom and bond properties without using the Chemifree codec."""
    atoms = [
        (
            atom.GetSymbol(),
            atom.GetFormalCharge(),
            atom.GetIsotope(),
            atom.GetNumRadicalElectrons(),
        )
        for atom in molecule.GetAtoms()
    ]
    bonds = [
        (bond.GetBeginAtomIdx(), bond.GetEndAtomIdx(), str(bond.GetBondType()))
        for bond in molecule.GetBonds()
    ]
    return atoms, bonds


def _verify_acetate(child: subprocess.Popen[str]) -> None:
    source = (
        ROOT / "crates/providers/tests/data/molfile/sodium-acetate.mol"
    ).read_text(encoding="utf-8")
    expected = (
        [
            ("C", 0, 0, 0),
            ("C", 0, 0, 0),
            ("O", 0, 0, 0),
            ("O", -1, 0, 0),
            ("Na", 1, 0, 0),
        ],
        [(0, 1, "SINGLE"), (1, 2, "DOUBLE"), (1, 3, "SINGLE")],
    )
    if _describe(_decode(source)) != expected:
        raise RuntimeError("RDKit disagreed with the sodium acetate fixture")
    imported = _request(
        child,
        "acetate-in",
        "document.import",
        {"format": "mol", "text": source},
    )
    document = imported["result"]["document"]
    atoms = [
        (a["element"], a["charge"], a["isotope"]) for a in document["atoms"]
    ]
    expected_atoms = [
        (s, c, isotope or None) for s, c, isotope, _ in expected[0]
    ]
    if atoms != expected_atoms:
        raise RuntimeError(f"Chemifree import atom mismatch: {atoms}")
    exported = _request(
        child,
        "acetate-out",
        "document.export",
        {"format": "mol", "document_id": document["id"]},
    )
    decoded = _decode(exported["result"]["text"])
    if _describe(decoded) != expected:
        raise RuntimeError("RDKit disagreed with Chemifree's acetate export")
    positions = [
        (
            round(decoded.GetConformer().GetAtomPosition(i).x, 4),
            round(decoded.GetConformer().GetAtomPosition(i).y, 4),
        )
        for i in range(5)
    ]
    expected_positions = [
        (-1.5, 0.0),
        (0.0, 0.0),
        (0.75, 1.299),
        (0.75, -1.299),
        (3.0, -1.299),
    ]
    if positions != expected_positions:
        raise RuntimeError(f"RDKit export coordinates differ: {positions}")
    print("Sodium acetate: atoms, bonds, charges and coordinates matched")


def _verify_rdkit_inputs(child: subprocess.Popen[str]) -> None:
    for number, smiles in enumerate(("c1ccccc1", "[13CH3][O-]"), 1):
        molecule = Chem.MolFromSmiles(smiles)
        AllChem.Compute2DCoords(molecule)
        source = Chem.MolToMolBlock(molecule, forceV3000=False)
        reference = _decode(source)
        imported = _request(
            child,
            f"rdkit-{number}",
            "document.import",
            {"format": "mol", "text": source},
        )
        document = imported["result"]["document"]
        atoms, bonds = _describe(reference)
        actual_atoms = [
            (a["element"], a["charge"], a["isotope"] or 0)
            for a in document["atoms"]
        ]
        if actual_atoms != [(s, c, isotope) for s, c, isotope, _ in atoms]:
            raise RuntimeError(f"{smiles}: atom mismatch {actual_atoms}")
        actual_bonds = [
            (int(b["begin"][1:]) - 1, int(b["end"][1:]) - 1, b["order"].upper())
            for b in document["bonds"]
        ]
        if actual_bonds != bonds:
            raise RuntimeError(
                f"{smiles}: bond mismatch {actual_bonds} {bonds}"
            )
        for index, atom in enumerate(document["atoms"]):
            position = reference.GetConformer().GetAtomPosition(index)
            if (
                abs(atom["position"]["x"] - position.x * 16) > 1e-9
                or abs(atom["position"]["y"] + position.y * 16) > 1e-9
            ):
                raise RuntimeError(f"{smiles}: coordinate mismatch at {index}")
        print(f"RDKit-written {smiles}: graph and coordinates matched")


def _verify_compact_isotopes(child: subprocess.Popen[str]) -> None:
    compact = (
        "Compact isotope fixture\n\n\n"
        "  2  0  0  0  0  0  0  0  0  0999 V2000\n"
        "    0.0000    0.0000    0.0000 Ni  1  0  0  0  0  0"
        "  0  0  0  0  0  0\n"
        "    1.0000    0.0000    0.0000 Br  1  0  0  0  0  0"
        "  0  0  0  0  0  0\n"
        "M  END\n"
    )
    reference = [atom.GetIsotope() for atom in _decode(compact).GetAtoms()]
    if reference != [59, 80]:
        raise RuntimeError(
            f"Unexpected RDKit compact isotope result: {reference}"
        )
    refused = _request(
        child,
        "compact",
        "document.import",
        {"format": "mol", "text": compact},
        allow_error=True,
    )
    if (
        refused["outcome"] != "error"
        or refused["diagnostics"][0]["code"] != "unsupported_isotope_reference"
    ):
        raise RuntimeError(
            f"Ambiguous compact isotopes were not refused: {refused}"
        )
    explicit = compact.replace("M  END", "M  ISO  2   1  60   2  81\nM  END")
    read_back = _request(
        child,
        "absolute",
        "document.import",
        {"format": "mol", "text": explicit},
    )
    observed = [
        atom["isotope"] for atom in read_back["result"]["document"]["atoms"]
    ]
    independent = [atom.GetIsotope() for atom in _decode(explicit).GetAtoms()]
    if observed != [60, 81] or independent != observed:
        raise RuntimeError(f"Explicit Ni/Br isotope mismatch: {observed}")
    print("Ni/Br: ambiguous compact values refused; explicit isotopes matched")


def _verify_exported_attributes(child: subprocess.Popen[str]) -> None:
    created = _request(
        child, "attr-create", "document.create", {"title": "Attributes"}
    )
    document_id = created["result"]["document"]["id"]
    _request(
        child,
        "attr-graph",
        "document.transaction",
        {
            "document_id": document_id,
            "expected_revision": 0,
            "commands": [
                {
                    "op": "add_atom",
                    "element": "C",
                    "position": {"x": 0, "y": 0},
                },
                {
                    "op": "add_atom",
                    "element": "O",
                    "position": {"x": 16, "y": 0},
                },
                {
                    "op": "add_bond",
                    "begin": "$0",
                    "end": "$1",
                    "order": "query",
                    "query": {"kind": "list", "orders": ["single", "double"]},
                },
            ],
        },
    )
    _request(
        child,
        "attr-state",
        "document.transaction",
        {
            "document_id": document_id,
            "expected_revision": 1,
            "commands": [
                {
                    "op": "update_atom",
                    "atom": "a1",
                    "charge": 4,
                    "isotope": 13,
                    "radical": "triplet",
                },
            ],
        },
    )
    exported = _request(
        child,
        "attr-out",
        "document.export",
        {
            "format": "mol",
            "document_id": document_id,
        },
    )
    molecule = _decode(exported["result"]["text"])
    atom = molecule.GetAtomWithIdx(0)
    state = (
        atom.GetFormalCharge(),
        atom.GetIsotope(),
        atom.GetNumRadicalElectrons(),
    )
    if state != (4, 13, 2):
        raise RuntimeError(f"Charge/isotope/radical mismatch: {state}")
    query = molecule.GetBondWithIdx(0)
    if not query.HasQuery():
        raise RuntimeError("RDKit did not decode a query bond")
    matched = [
        query.Match(Chem.MolFromSmiles(smiles).GetBondWithIdx(0))
        for smiles in ("CC", "C=C", "C#C")
    ]
    if matched != [True, True, False]:
        raise RuntimeError(f"Query-bond semantics differ: {matched}")
    print("RDKit decoded charge, isotope, triplet and single-or-double query")


def _verify_sdf_and_padded_end(child: subprocess.Popen[str]) -> None:
    source = (
        ROOT / "crates/providers/tests/data/molfile/sodium-acetate.mol"
    ).read_text(encoding="utf-8")
    padded = source.replace("M  END\n", "M  END   \n")
    padded_molecule = Chem.MolFromMolBlock(
        padded, sanitize=False, removeHs=False, strictParsing=True
    )
    print(f"RDKit accepts space-padded M  END: {padded_molecule is not None}")
    first = _request(
        child, "sdf-one", "document.import", {"format": "mol", "text": padded}
    )["result"]["document"]["id"]
    benzene = Chem.MolFromSmiles("c1ccccc1")
    benzene.SetProp("_Name", "Benzene")
    AllChem.Compute2DCoords(benzene)
    second = _request(
        child,
        "sdf-two",
        "document.import",
        {
            "format": "mol",
            "text": Chem.MolToMolBlock(benzene, forceV3000=False),
        },
    )["result"]["document"]["id"]
    reply = _request(
        child,
        "sdf-out",
        "document.export",
        {
            "format": "sdf",
            "documents": [
                {
                    "document_id": first,
                    "fields": [{"name": "NOTE", "value": "first\nsecond"}],
                },
                {
                    "document_id": second,
                    "fields": [{"name": "NOTE", "value": "benzene"}],
                },
            ],
        },
    )
    text = reply["result"]["text"]
    if [line for line in text.splitlines() if line.startswith("$$$$")] != [
        "$$$$",
        "$$$$",
    ]:
        raise RuntimeError("SDF export did not use two canonical delimiters")
    supplier = Chem.ForwardSDMolSupplier(
        io.BytesIO(text.encode("utf-8")),
        sanitize=False,
        removeHs=False,
        strictParsing=True,
    )
    molecules = list(supplier)
    if (
        len(molecules) != 2
        or any(molecule is None for molecule in molecules)
        or [molecule.GetNumAtoms() for molecule in molecules] != [5, 6]
        or [molecule.GetProp("NOTE") for molecule in molecules]
        != ["first\nsecond", "benzene"]
    ):
        raise RuntimeError(
            "RDKit did not read both exported SDF graphs and fields"
        )
    print("RDKit read two exported SDF records and their separate data values")


def _verify_alkene_stereo_uncertainty(child: subprocess.Popen[str]) -> None:
    observed = []
    for suffix, last_y in (("same", 16), ("opposite", -16)):
        created = _request(
            child,
            f"alkene-create-{suffix}",
            "document.create",
            {"title": "2-butene"},
        )
        document_id = created["result"]["document"]["id"]
        points = [(-16, 16), (0, 0), (16, 0), (32, last_y)]
        commands = [
            {"op": "add_atom", "element": "C", "position": {"x": x, "y": y}}
            for x, y in points
        ]
        commands.extend(
            {
                "op": "add_bond",
                "begin": f"${index}",
                "end": f"${index + 1}",
                "order": order,
            }
            for index, order in enumerate(("single", "double", "single"))
        )
        _request(
            child,
            f"alkene-graph-{suffix}",
            "document.transaction",
            {
                "document_id": document_id,
                "expected_revision": 0,
                "commands": commands,
            },
        )
        exported = _request(
            child,
            f"alkene-out-{suffix}",
            "document.export",
            {"format": "mol", "document_id": document_id},
        )
        if not any(
            item["feature"] == "coordinate_stereo"
            for item in exported["loss_report"]
        ):
            raise RuntimeError(
                "Export omitted its coordinate-stereo uncertainty"
            )
        block = exported["result"]["text"]
        if [line[9:12] for line in block.splitlines()[8:11]] != ["  0"] * 3:
            raise RuntimeError("Exported alkene bonds contained a stereo code")
        molecule = Chem.MolFromMolBlock(
            block, sanitize=True, removeHs=False, strictParsing=True
        )
        if molecule is None:
            raise RuntimeError("RDKit rejected the exported alkene")
        observed.append(str(molecule.GetBondBetweenAtoms(1, 2).GetStereo()))
    if set(observed) != {"STEREOE", "STEREOZ"}:
        raise RuntimeError(
            f"RDKit did not distinguish the two alkene geometries: {observed}"
        )
    print(
        "RDKit inferred distinct double-bond stereo from exported coordinates: "
        f"{observed}"
    )


def main() -> None:
    """Run the independent checks against an isolated CLI data directory."""
    print(f"Independent reader: RDKit {rdBase.rdkitVersion}")
    with (
        tempfile.TemporaryDirectory(prefix="chemifree-rdkit-") as data,
        # The executable is the locally built CLI selected by CARGO_TARGET_DIR.
        subprocess.Popen(  # noqa: S603
            [str(BINARY), "--data-dir", data, "operations"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        ) as child,
    ):
        _verify_acetate(child)
        _verify_rdkit_inputs(child)
        _verify_compact_isotopes(child)
        _verify_exported_attributes(child)
        _verify_sdf_and_padded_end(child)
        _verify_alkene_stereo_uncertainty(child)


if __name__ == "__main__":
    main()
