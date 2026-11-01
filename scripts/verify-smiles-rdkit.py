"""Compare Chemifree SMILES reading and writing with an RDKit 2022.03.2 Python.

Run after building the smiles_reference example and pass its absolute path:

    python -I scripts/verify-smiles-rdkit.py /path/to/smiles_reference

Every fixture states its expected outcome for both writers: the exact text, or a specific
refusal code. A fixture that produces anything else, including an error, fails; nothing
passes silently. For each text Chemifree writes, RDKit must read the same molecule as it reads
from the fixture itself. Chemifree's canonical text must be a fixpoint, and the spellings in a
group must give one canonical text or the same refusal. Fixtures without aromatic atoms are also
compared atom by atom and bond by bond with RDKit's unsanitized reading of the input.
No network and no proprietary input is used.
"""

from __future__ import annotations

import dataclasses
import json
import logging
import pathlib
import subprocess
import sys
from typing import Any

import rdkit
import rdkit.Chem

_LOGGER = logging.getLogger(__name__)
_EXPECTED_RDKIT = "2022.03.2"


@dataclasses.dataclass(frozen=True)
class Refused:
    """An expected refusal, by the code Chemifree reports."""

    code: str


Expectation = str | Refused


@dataclasses.dataclass(frozen=True)
class Fixture:
    smiles: str
    recorded: Expectation
    canonical: Expectation
    # Whether to compare the reader's atoms and bonds with RDKit's unsanitized reading.
    structure: bool = True
    # RDKit rejects some valid OpenSMILES, such as isotope digits with leading zeros. The
    # spelling it reads instead is stated, so the comparison is explicit about the difference.
    rdkit_input: str | None = None


def _fixtures() -> list[Fixture]:
    canonical_refused = Refused("canonical_unsupported")
    return [
        Fixture("CCO", "CCO", "C(C)O"),
        Fixture("OCC", "OCC", "C(C)O"),
        Fixture("C(O)C", "C(O)C", "C(C)O"),
        Fixture("CC(=O)O", "CC(=O)O", "C(C)(=O)O"),
        Fixture("OC(C)=O", "OC(C)=O", "C(C)(=O)O"),
        Fixture("CC(C)(C)C", "CC(C)(C)C", "C(C)(C)(C)C"),
        Fixture("C1CCCCC1", "C1CCCCC1", "C1CCCCC1"),
        Fixture("C1CC1C1CC1", "C1CC1C1CC1", "C1(CC1)C1CC1"),
        Fixture("C12(CCCCC1)CCCCC2", "C12(CCCCC1)CCCCC2", "C12(CCCCC2)CCCCC1"),
        Fixture("C1CC11CC1", "C1CC11CC1", "C12(CC2)CC1"),
        Fixture("C%12CCCCC%12", "C1CCCCC1", "C1CCCCC1"),
        Fixture("C1CC2CCC1C2", "C1CC2CCC1C2", "C12CC(CC1)CC2"),
        Fixture("CC1=CCC2CC1C2(C)C", "CC1=CCC2CC1C2(C)C", "C1(C2C(=CCC1C2)C)(C)C"),
        Fixture("[Na+].[Cl-]", "[Na+].[Cl-]", canonical_refused),
        Fixture("[NH4+]", "[NH4+]", "[NH4+]"),
        Fixture("[CH3]", "[CH3]", "[CH3]"),
        Fixture("[OH]", "[OH]", "[OH]"),
        Fixture("[CH2]", "[CH2]", canonical_refused),
        Fixture("[2H]C([2H])([2H])[2H]", "[2H]C([2H])([2H])[2H]", "[2H]C([2H])([2H])[2H]"),
        Fixture("[002H]C", "[2H]C", "[2H]C", structure=False, rdkit_input="[2H]C"),
        Fixture(
            "[002H]C([2H])([2H])[2H]",
            "[2H]C([2H])([2H])[2H]",
            "[2H]C([2H])([2H])[2H]",
            structure=False,
            rdkit_input="[2H]C([2H])([2H])[2H]",
        ),
        Fixture("[H][H]", "[H][H]", "[H][H]"),
        Fixture("C[H]", "C[H]", "C"),
        Fixture("CCO.CC(=O)O", "CCO.CC(=O)O", "C(C)(=O)O.C(C)O"),
        Fixture("N#CC(=O)O", "N#CC(=O)O", "C(C#N)(=O)O"),
        Fixture("c1ccccc1", "c1ccccc1", "c1ccccc1", structure=False),
        Fixture("C1=CC=CC=C1", "C1=CC=CC=C1", "c1ccccc1"),
        Fixture("c1ccccc1c1ccccc1", "c1ccccc1-c1ccccc1", "c1(ccccc1)-c1ccccc1", structure=False),
        Fixture("c1cc[nH]c1", "c1cc[nH]c1", "c1c[nH]cc1", structure=False),
        Fixture("C1=CNC=C1", "C1=CNC=C1", "c1c[nH]cc1"),
        Fixture("c1ccncc1", "c1ccncc1", "c1ccncc1", structure=False),
        Fixture("c1ccc2ccccc2c1", "c1ccc2ccccc2c1", "c12ccccc1cccc2", structure=False),
        Fixture("[O-]c1ccccc1", "[O-]c1ccccc1", "c1(ccccc1)[O-]", structure=False),
        Fixture("C1=CC=C2CCCCC2=C1", "C1=CC=C2CCCCC2=C1", canonical_refused),
        Fixture("c1ccc2c(c1)CCCC2", Refused("aromatic_system_unsupported"), Refused("aromatic_system_unsupported")),
        Fixture("F/C=C/F", Refused("stereo_unsupported"), Refused("stereo_unsupported")),
        Fixture("C=(O)", Refused("dangling_bond"), Refused("dangling_bond")),
        Fixture("c1ccccc1:c1ccccc1", Refused("aromatic_bond_outside_ring"), Refused("aromatic_bond_outside_ring")),
    ]


# Spellings of one graph: their canonical results must agree.
_GROUPS = [
    ["CCO", "OCC", "C(O)C"],
    ["CC(=O)O", "OC(C)=O"],
    ["c1ccccc1", "C1=CC=CC=C1"],
    ["c1cc[nH]c1", "C1=CNC=C1"],
    ["[2H]C([2H])([2H])[2H]", "[002H]C([2H])([2H])[2H]"],
    ["C1CCCCC1", "C%12CCCCC%12"],
    ["C1=CC=C2CCCCC2=C1", "c1ccc2c(c1)CCCC2"],
]


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _run(binary: pathlib.Path, *arguments: str) -> dict[str, Any]:
    # The caller explicitly supplies the local executable; no shell is involved.
    output = subprocess.run(  # noqa: S603
        [str(binary), *arguments],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    result: object = json.loads(output.stdout)
    if not isinstance(result, dict) or not all(isinstance(k, str) for k in result):
        raise ValueError("The Chemifree response must be a JSON object.")
    return result


def _rdkit_canonical(smiles: str) -> str | None:
    molecule = rdkit.Chem.MolFromSmiles(smiles)
    if molecule is None:
        return None
    return str(rdkit.Chem.MolToSmiles(molecule, isomericSmiles=False))


def _outcome(
    binary: pathlib.Path, style: str, smiles: str
) -> str | Refused:
    answer = _run(binary, "write", style, smiles)
    if answer["outcome"] == "text":
        text = answer["text"]
        _require(isinstance(text, str), f"{smiles}: the text must be a string")
        return str(text)
    _require(answer["outcome"] == "refused", f"{smiles}: unexpected answer {answer}")
    return Refused(str(answer["code"]))


def _check_expected(
    fixture: Fixture, style: str, expected: Expectation, found: str | Refused
) -> None:
    _require(
        found == expected,
        f"{fixture.smiles}: {style} expected {expected!r}, found {found!r}",
    )


def _check_structure(binary: pathlib.Path, fixture: Fixture) -> None:
    """Chemifree's reading of the input equals RDKit's unsanitized reading."""
    answer = _run(binary, "read", fixture.smiles)
    _require(answer["outcome"] == "read", f"{fixture.smiles}: {answer}")
    molecule = rdkit.Chem.MolFromSmiles(fixture.smiles, sanitize=False)
    _require(molecule is not None, f"{fixture.smiles}: RDKit cannot read the input")
    atoms = answer["atoms"]
    _require(len(atoms) == molecule.GetNumAtoms(), f"{fixture.smiles}: atom count")
    for index, atom in enumerate(atoms):
        other = molecule.GetAtomWithIdx(index)
        _require(atom["element"] == other.GetSymbol(), f"{fixture.smiles}: element {index}")
        _require(atom["charge"] == other.GetFormalCharge(), f"{fixture.smiles}: charge {index}")
        _require((atom["isotope"] or 0) == other.GetIsotope(), f"{fixture.smiles}: isotope {index}")
        if atom["hydrogens"] is not None:
            _require(
                atom["hydrogens"] == other.GetNumExplicitHs(),
                f"{fixture.smiles}: hydrogens {index}",
            )
    ours = sorted(
        (min(a, b), max(a, b), order) for a, b, order in answer["bonds"]
    )
    names = {
        rdkit.Chem.BondType.SINGLE: "single",
        rdkit.Chem.BondType.DOUBLE: "double",
        rdkit.Chem.BondType.TRIPLE: "triple",
    }
    theirs = sorted(
        (
            min(bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()),
            max(bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()),
            names[bond.GetBondType()],
        )
        for bond in molecule.GetBonds()
    )
    _require(ours == theirs, f"{fixture.smiles}: bonds {ours} != {theirs}")


def _check_fixture(binary: pathlib.Path, fixture: Fixture) -> dict[str, str | Refused]:
    results: dict[str, str | Refused] = {}
    for style, expected in (("recorded", fixture.recorded), ("canonical", fixture.canonical)):
        found = _outcome(binary, style, fixture.smiles)
        _check_expected(fixture, style, expected, found)
        results[style] = found
        if isinstance(found, str):
            reference = _rdkit_canonical(fixture.rdkit_input or fixture.smiles)
            _require(reference is not None, f"{fixture.smiles}: RDKit cannot read the input")
            _require(
                _rdkit_canonical(found) == reference,
                f"{fixture.smiles}: RDKit reads {found!r} as a different molecule than the input",
            )
    canonical = results["canonical"]
    if isinstance(canonical, str):
        again = _outcome(binary, "canonical", canonical)
        _require(again == canonical, f"{fixture.smiles}: {canonical!r} is not a fixpoint ({again!r})")
    if fixture.structure and isinstance(results["recorded"], str):
        _check_structure(binary, fixture)
    return results


def _group_key(outcome: str | Refused) -> str:
    return "refused" if isinstance(outcome, Refused) else outcome


def main(argv: list[str]) -> int:
    """Check every fixture and spelling group; return the exit status."""
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    if len(argv) != 2:
        _LOGGER.error("usage: verify-smiles-rdkit.py /path/to/smiles_reference")
        return 2
    _require(
        rdkit.__version__ == _EXPECTED_RDKIT,
        f"RDKit {_EXPECTED_RDKIT} is required, found {rdkit.__version__}",
    )
    binary = pathlib.Path(argv[1])
    fixtures = _fixtures()
    outcomes = {fixture.smiles: _check_fixture(binary, fixture) for fixture in fixtures}
    for group in _GROUPS:
        for smiles in group:
            _require(smiles in outcomes, f"{smiles}: group member without a fixture")
        # One text for every spelling, or a refusal for every spelling; never a mix. Which
        # code refuses may differ, because the reader or the canonical form may be the one.
        shown = {_group_key(outcomes[smiles]["canonical"]) for smiles in group}
        _require(len(shown) == 1, f"{group}: spellings disagree: {sorted(shown)}")
    _LOGGER.info("checked %d fixtures and %d spelling groups", len(fixtures), len(_GROUPS))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
