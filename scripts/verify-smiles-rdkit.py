"""Compare the bounded SMILES reader and writer with RDKit 2022.03.2.

Pass the absolute path to the built ``smiles_reference`` example. This uses
only public, independently specified SMILES fixtures and creates no files.
Each fixture declares its expected outcome: by default the writer and canonical
output must succeed and agree with RDKit; a fixture may instead name the text a
specific refusal must contain, and any other outcome fails.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys
from typing import NamedTuple

import rdkit
import rdkit.Chem


class Fixture(NamedTuple):
    """A SMILES source and the outcome the reference binary must report.

    Attributes:
      source: The SMILES passed to the reference binary and to RDKit.
      write_refusal: Text the recorded writer's refusal must contain, if it is
        expected to refuse (the reader still accepts the source).
      canonical_refusal: Text the canonical writer's refusal must contain, if
        only canonical output is expected to refuse.
    """

    source: str
    write_refusal: str | None = None
    canonical_refusal: str | None = None


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _expected_order(bond: rdkit.Chem.Bond, sanitized: rdkit.Chem.Mol) -> str:
    """Return the bond type OpenSMILES gives a bond of the unsanitized parse.

    An unwritten bond between aromatic atoms is aromatic only inside a ring.
    RDKit's unsanitized parse leaves it aromatic, so use its ring perception.
    """
    order = str(bond.GetBondType())
    if order != "AROMATIC":
        return order
    ring_check = sanitized.GetBondBetweenAtoms(
        bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()
    )
    return order if ring_check.IsInRing() else "SINGLE"


def _compare(binary: pathlib.Path, fixture: Fixture) -> None:
    source = fixture.source
    # The caller explicitly selects this locally built test executable.
    completed = subprocess.run(  # noqa: S603
        [str(binary), source], check=False, capture_output=True, text=True
    )
    if fixture.write_refusal is not None:
        _require(
            completed.returncode == 1
            and fixture.write_refusal in completed.stderr,
            f"expected writer refusal {fixture.write_refusal!r}: {source}"
            f" (exit {completed.returncode}, {completed.stderr!r})",
        )
        return
    _require(
        completed.returncode == 0,
        f"reference binary failed ({completed.returncode}) for {source}:"
        f" {completed.stderr!r}",
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
    original = rdkit.Chem.MolFromSmiles(source)
    _require(original is not None, f"sanitization: {source}")
    expected_bonds = sorted(
        (
            min(b.GetBeginAtomIdx(), b.GetEndAtomIdx()),
            max(b.GetBeginAtomIdx(), b.GetEndAtomIdx()),
            _expected_order(b, original),
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
    written = rdkit.Chem.MolFromSmiles(output)
    _require(
        original is not None and written is not None, f"sanitization: {source}"
    )
    _require(
        rdkit.Chem.MolToSmiles(original, canonical=True)
        == rdkit.Chem.MolToSmiles(written, canonical=True),
        f"written graph differs: {source}",
    )
    canonical = [row[1] for row in records if row[0] == "canonical"]
    refusals = [row[1] for row in records if row[0] == "canonical_error"]
    if fixture.canonical_refusal is not None:
        _require(
            not canonical
            and len(refusals) == 1
            and fixture.canonical_refusal in refusals[0],
            f"expected canonical refusal {fixture.canonical_refusal!r}:"
            f" {source} (got {canonical or refusals})",
        )
        return
    _require(
        len(canonical) == 1 and not refusals,
        f"canonical output was not produced for {source}: {refusals}",
    )
    decoded = rdkit.Chem.MolFromSmiles(canonical[0])
    _require(
        decoded is not None, f"canonical output refused by RDKit: {source}"
    )
    _require(
        rdkit.Chem.MolToSmiles(original, canonical=True)
        == rdkit.Chem.MolToSmiles(decoded, canonical=True),
        f"canonical output changed chemistry: {source}",
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
    fixtures = [
        Fixture(source)
        for source in (
            "CC(O)C",
            "C1=CC=CC=C1",
            "c1ccccc1",
            "[nH]1cccc1",
            "[nH+]1ccccc1",
            "c1ccc2ccccc2c1",
            "[13CH3-].[NH4+]",
            "[13CH4]",
            "[OH-]",
            "[2H]O[2H]",
            "C1CC11CC1",
            "c1ccccc1-c1ccccc1",
            "c1ccccc1c1ccccc1",
            "C1CC1",
            "[CH2]1[CH2][CH2]1",
            "CC1(C)CC1",
            "C1CCC2(CC1)CCCC2",
        )
    ]
    # Sodium is outside the shared valence model, so its radical state is
    # undetermined and the writer refuses rather than guessing.
    fixtures.append(Fixture("[Na+].[Cl-]", write_refusal="UnsupportedAtom"))
    for fixture in fixtures:
        _compare(binary, fixture)
    sys.stdout.write(f"RDKit {rdkit.__version__}: SMILES comparisons passed\n")


if __name__ == "__main__":
    main()
