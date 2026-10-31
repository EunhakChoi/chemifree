"""Compare the bounded SMILES reader and writer with RDKit 2022.03.2.

Pass the absolute path to the built ``smiles_reference`` example. This uses
only public, independently specified SMILES fixtures and creates no files.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

import rdkit
import rdkit.Chem


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _compare(binary: pathlib.Path, source: str) -> None:
    # The caller explicitly selects this locally built test executable.
    completed = subprocess.run(  # noqa: S603
        [str(binary), source], check=True, capture_output=True, text=True
    )
    records = [line.split("\t") for line in completed.stdout.splitlines()]
    _require(bool(records) and records[0][0] == "smiles", "missing output")
    output = records[0][1]
    reference = rdkit.Chem.MolFromSmiles(source, sanitize=False)
    _require(reference is not None, f"RDKit refused {source}")
    atoms = [row for row in records if row[0] == "atom"]
    bonds = [row for row in records if row[0] == "bond"]
    _require(len(atoms) == reference.GetNumAtoms(), f"atom count: {source}")
    _require(len(bonds) == reference.GetNumBonds(), f"bond count: {source}")
    for row, atom in zip(atoms, reference.GetAtoms(), strict=True):
        observed = (row[1], int(row[2]), int(row[3]), row[5] == "true")
        expected = (
            atom.GetSymbol(),
            atom.GetIsotope(),
            atom.GetFormalCharge(),
            atom.GetIsAromatic(),
        )
        _require(observed == expected, f"atom state: {source}")
        if row[4] != "inferred":
            _require(
                int(row[4]) == atom.GetNumExplicitHs(), f"hydrogen: {source}"
            )
    expected_bonds = sorted(
        (
            min(b.GetBeginAtomIdx(), b.GetEndAtomIdx()),
            max(b.GetBeginAtomIdx(), b.GetEndAtomIdx()),
            str(b.GetBondType()),
        )
        for b in reference.GetBonds()
    )
    actual_bonds = sorted(
        (
            min(int(row[1]), int(row[2])),
            max(int(row[1]), int(row[2])),
            row[3].upper(),
        )
        for row in bonds
    )
    _require(actual_bonds == expected_bonds, f"bond state: {source}")
    original = rdkit.Chem.MolFromSmiles(source)
    written = rdkit.Chem.MolFromSmiles(output)
    _require(
        original is not None and written is not None, f"sanitization: {source}"
    )
    _require(
        rdkit.Chem.MolToSmiles(original, canonical=True)
        == rdkit.Chem.MolToSmiles(written, canonical=True),
        f"written graph differs: {source}",
    )


def main() -> None:
    """Check pinned independent reader, writer, and graph fixtures."""
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: verify-smiles-rdkit.py /path/to/smiles_reference"
        )
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    _require(rdkit.__version__ == "2022.03.2", "reference version changed")
    carbon = rdkit.Chem.MolFromSmiles("[C]")
    _require(carbon is not None, "RDKit refused the bracket carbon fixture")
    _require(
        carbon.GetAtomWithIdx(0).GetNumRadicalElectrons() == 4,
        "RDKit's bracket carbon radical interpretation changed",
    )
    for source in (
        "CC(O)C",
        "C1=CC=CC=C1",
        "c1ccccc1",
        "[nH]1cccc1",
        "[13CH3-].[NH4+]",
        "C1CC11CC1",
        "c1ccccc1-c1ccccc1",
    ):
        _compare(binary, source)
    sys.stdout.write(f"RDKit {rdkit.__version__}: SMILES comparisons passed\n")


if __name__ == "__main__":
    main()
