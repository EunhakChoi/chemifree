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
        canonical output is expected to refuse.
      alternates: Other spellings of the same molecule; each must give the
        identical Chemifree canonical outcome as `source`.
    """

    source: str
    write_refusal: str | None = None
    canonical_refusal: str | None = None
    alternates: tuple[str, ...] = ()


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


def _run(binary: pathlib.Path, source: str) -> list[list[str]]:
    """Run the reference binary and return its tab-separated records."""
    # The caller explicitly selects this locally built test executable.
    completed = subprocess.run(  # noqa: S603
        [str(binary), source], check=False, capture_output=True, text=True
    )
    _require(
        completed.returncode == 0,
        f"reference binary failed ({completed.returncode}) for {source}:"
        f" {completed.stderr!r}",
    )
    return [line.split("\t") for line in completed.stdout.splitlines()]


def _canonical_outcome(
    records: list[list[str]], source: str
) -> tuple[str, str]:
    """Return ("text", canonical) or ("refused", reason) from the records."""
    canonical = [row[1] for row in records if row[0] == "canonical"]
    refusals = [row[1] for row in records if row[0] == "canonical_error"]
    _require(
        len(canonical) + len(refusals) == 1,
        f"expected exactly one canonical outcome for {source}: {records}",
    )
    return ("text", canonical[0]) if canonical else ("refused", refusals[0])


def _compare_reader_state(
    records: list[list[str]], source: str, original: rdkit.Chem.Mol
) -> None:
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


def _compare(binary: pathlib.Path, fixture: Fixture) -> None:
    source = fixture.source
    records = _run(binary, source)
    original = rdkit.Chem.MolFromSmiles(source)
    _require(original is not None, f"sanitization: {source}")
    # Reader state is compared even when the writer is expected to refuse.
    _compare_reader_state(records, source, original)
    outputs = [row[1] for row in records if row[0] == "smiles"]
    write_errors = [row[1] for row in records if row[0] == "write_error"]
    if fixture.write_refusal is not None:
        _require(
            not outputs
            and len(write_errors) == 1
            and fixture.write_refusal in write_errors[0],
            f"expected writer refusal {fixture.write_refusal!r}: {source}"
            f" (got {outputs or write_errors})",
        )
    else:
        _require(
            len(outputs) == 1 and not write_errors,
            f"writer did not produce one output for {source}: {write_errors}",
        )
        written = rdkit.Chem.MolFromSmiles(outputs[0])
        _require(written is not None, f"sanitization of output: {source}")
        _require(
            rdkit.Chem.MolToSmiles(original, canonical=True)
            == rdkit.Chem.MolToSmiles(written, canonical=True),
            f"written graph differs: {source}",
        )
    kind, value = _canonical_outcome(records, source)
    if fixture.canonical_refusal is not None:
        _require(
            kind == "refused" and fixture.canonical_refusal in value,
            f"expected canonical refusal {fixture.canonical_refusal!r}:"
            f" {source} (got {kind}: {value})",
        )
    else:
        _require(
            kind == "text",
            f"canonical output was not produced for {source}: {value}",
        )
        decoded = rdkit.Chem.MolFromSmiles(value)
        _require(
            decoded is not None, f"canonical output refused by RDKit: {source}"
        )
        _require(
            rdkit.Chem.MolToSmiles(original, canonical=True)
            == rdkit.Chem.MolToSmiles(decoded, canonical=True),
            f"canonical output changed chemistry: {source}",
        )
        # Chemifree's own canonical text must be a fixpoint of its reader.
        again = _canonical_outcome(_run(binary, value), value)
        _require(
            again == ("text", value),
            f"canonical text is not a fixpoint: {source} -> {value} -> {again}",
        )
    # Alternate spellings of one molecule agree with the base spelling.
    for alternate in fixture.alternates:
        _require(
            _canonical_outcome(_run(binary, alternate), alternate)
            == (kind, value),
            f"spelling {alternate!r} disagrees with {source!r}",
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
        Fixture("CC(O)C", alternates=("C(C)(O)C", "OC(C)C")),
        Fixture("C1=CC=CC=C1", alternates=("c1ccccc1", "C=1C=CC=CC=1")),
        Fixture("c1ccccc1", alternates=("C1=CC=CC=C1",)),
        Fixture(
            "[nH]1cccc1",
            alternates=("N1C=CC=C1", "C1=CNC=C1", "[NH]1C=CC=C1"),
        ),
        Fixture("N1C=CC=C1", alternates=("[nH]1cccc1",)),
        Fixture("N1C=CN=C1", alternates=("c1c[nH]cn1", "C1=CN=CN1")),
        Fixture("N1=CC=CC=C1", alternates=("c1ccncc1",)),
        Fixture("[nH+]1ccccc1"),
        Fixture(
            "c1ccc2ccccc2c1",
            alternates=("C1=CC2=CC=CC=C2C=C1", "C1=CC=C2C=CC=CC2=C1"),
        ),
        Fixture("[13CH3-].[NH4+]", alternates=("[NH4+].[13CH3-]",)),
        Fixture("[13CH4]"),
        Fixture("[OH-]"),
        Fixture("[2H]O[2H]"),
        Fixture("C1CC11CC1"),
        Fixture("c1ccccc1-c1ccccc1", alternates=("c1ccccc1c1ccccc1",)),
        Fixture("C1CC1", alternates=("[CH2]1[CH2][CH2]1",)),
        Fixture("CC1(C)CC1", alternates=("C1CC1(C)C",)),
        Fixture("C1CCC2(CC1)CCCC2", alternates=("C1CCC2(CCCCC2)C1",)),
        Fixture("C1CC2CCC1C2"),
        Fixture("C1CCC2CCCCC2C1"),
        # Fused systems with double bonds that are not aromatic as a whole are
        # refused for every spelling instead of depending on the Kekule form.
        Fixture(
            "C1CC2=CC=CC=C2C1",
            canonical_refusal="fused_system_outside_model",
            alternates=("C1CC2=C(C1)C=CC=C2",),
        ),
        Fixture(
            "C1CCC2=CC=CC=C2C1",
            canonical_refusal="fused_system_outside_model",
        ),
    ]
    # Sodium is outside the shared valence model, so its radical state is
    # undetermined and both writers refuse rather than guess.
    fixtures.append(
        Fixture(
            "[Na+].[Cl-]",
            write_refusal="UnsupportedAtom",
            canonical_refusal="Unsupported",
        )
    )
    for fixture in fixtures:
        _compare(binary, fixture)
    sys.stdout.write(f"RDKit {rdkit.__version__}: SMILES comparisons passed\n")


if __name__ == "__main__":
    main()
