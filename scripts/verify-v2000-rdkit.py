"""Compare independent RDKit 2022.03.2 and Chemifree V2000 interpretations.

Run after building ctfile_reference and pass its absolute path. Fixtures are
temporary, and no network or proprietary inputs are used.
"""

from __future__ import annotations

import json
import logging
import pathlib
import subprocess
import sys
import tempfile
from typing import Any

import rdkit
import rdkit.Chem

_LOGGER = logging.getLogger(__name__)


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _read_with_chemifree(
    binary: pathlib.Path, path: pathlib.Path
) -> dict[str, Any]:
    # The caller explicitly supplies the local executable; no shell is involved.
    output = subprocess.run(  # noqa: S603
        [str(binary), "read", str(path)],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    # This is a dynamic protocol response; the checks below inspect its fields.
    result: object = json.loads(output.stdout)
    if not isinstance(result, dict) or not all(
        isinstance(k, str) for k in result
    ):
        raise ValueError("The Chemifree response must be a JSON object.")
    return result


def _butene(last_y: int) -> str:
    # No wedge/hash flags: opposing coordinates alone distinguish cis/trans.
    return (
        "2-butene\n\n\n"
        "  4  3  0  0  0  0  0  0  0  0999 V2000\n"
        "   -1.0000    1.0000    0.0000 C"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "    0.0000    0.0000    0.0000 C"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "    1.0000    0.0000    0.0000 C"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        f"    2.0000{float(last_y):10.4f}    0.0000 C"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "  1  2  1  0  0  0  0\n"
        "  2  3  2  0  0  0  0\n"
        "  3  4  1  0  0  0  0\n"
        "M  END\n"
    )


def _hydrogen_defaults() -> str:
    return (
        "Defaults\n\n\n  3  0  0  0  0  0  0  0  0  0999 V2000\n"
        "    0.0000    0.0000    0.0000 N"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "    1.0000    0.0000    0.0000 C"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "    2.0000    0.0000    0.0000 O"
        "   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "M  CHG  1   1   1\nM  RAD  1   2   2\nM  END\n"
    )


def _check_coordinate_stereo(binary: pathlib.Path, root: pathlib.Path) -> None:
    for last_y, expected in [(1, "STEREOZ"), (-1, "STEREOE")]:
        source = _butene(last_y)
        reference_mol = rdkit.Chem.MolFromMolBlock(source, removeHs=False)
        _require(reference_mol is not None, "RDKit refused 2-butene")
        stereo_observation = str(reference_mol.GetBondWithIdx(1).GetStereo())
        _require(
            stereo_observation == expected,
            f"coordinate stereo: expected {expected}, got {stereo_observation}",
        )
        path = root / f"butene-{last_y}.mol"
        path.write_text(source, encoding="utf-8")
        parsed_butene = _read_with_chemifree(binary, path)
        _require(
            any(
                item["feature"] == "coordinate_stereo"
                for item in parsed_butene["losses"]
            ),
            "coordinate stereo uncertainty was omitted",
        )


def main() -> None:
    """Run reciprocal graph and coordinate-stereo checks against RDKit."""
    logging.basicConfig(
        level=logging.INFO, format="%(message)s", stream=sys.stdout
    )
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: verify-v2000-rdkit.py /absolute/path/to/ctfile_reference"
        )
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    _LOGGER.info("RDKit version: %s", rdkit.__version__)
    _require(rdkit.__version__ == "2022.03.2", "reference version changed")
    with tempfile.TemporaryDirectory(prefix="chemifree-rdkit-") as directory:
        root = pathlib.Path(directory)
        # The executable is explicitly selected by the caller; no shell is used.
        subprocess.run(  # noqa: S603
            [str(binary), "write", str(root)], check=True
        )
        mol = rdkit.Chem.MolFromMolFile(
            str(root / "molecule.mol"), sanitize=False, removeHs=False
        )
        _require(mol is not None, "RDKit refused the Chemifree MOL")
        _require(
            [
                (a.GetSymbol(), a.GetIsotope(), a.GetFormalCharge())
                for a in mol.GetAtoms()
            ]
            == [("C", 13, 0), ("O", 0, -1)],
            "RDKit atom interpretation differs",
        )
        _require(
            [
                (b.GetBeginAtomIdx(), b.GetEndAtomIdx(), str(b.GetBondType()))
                for b in mol.GetBonds()
            ]
            == [(0, 1, "SINGLE")],
            "RDKit bond interpretation differs",
        )
        records = list(
            rdkit.Chem.SDMolSupplier(
                str(root / "records.sdf"), sanitize=False, removeHs=False
            )
        )
        _require(
            len(records) == 2 and all(item is not None for item in records),
            "RDKit did not read both SDF records",
        )
        _require(
            records[1].GetProp("Label") == "independent reader",
            "SDF value changed",
        )
        reference = rdkit.Chem.MolFromSmiles("[13CH3]CO")
        _require(
            reference is not None, "RDKit could not build the reference graph"
        )
        reference_path = root / "rdkit.mol"
        reference_path.write_text(
            rdkit.Chem.MolToMolBlock(reference), encoding="utf-8"
        )
        parsed = _read_with_chemifree(binary, reference_path)["records"][0]
        _require(
            [(a["element"], a["isotope"], a["charge"]) for a in parsed["atoms"]]
            == [("C", 13, 0), ("C", None, 0), ("O", None, 0)],
            "Chemifree interpreted RDKit's atoms incorrectly",
        )
        _require(
            [(b["begin"], b["end"], b["order"]) for b in parsed["bonds"]]
            == [(0, 1, 1), (1, 2, 1)],
            "Chemifree interpreted RDKit's bonds incorrectly",
        )
        # The bounded codec must refuse the anion's explicit valence rather than
        # silently interpreting it as default.
        explicit_valence = rdkit.Chem.MolFromSmiles("[13CH3][CH2][O-]")
        _require(
            explicit_valence is not None, "RDKit could not build the anion"
        )
        valence_path = root / "rdkit-explicit-valence.mol"
        valence_path.write_text(
            rdkit.Chem.MolToMolBlock(explicit_valence), encoding="utf-8"
        )
        # The selected local executable is called directly, without a shell.
        refusal = subprocess.run(  # noqa: S603
            [str(binary), "read", str(valence_path)],
            capture_output=True,
            text=True,
            check=False,
        )
        _require(
            refusal.returncode != 0
            and "unsupported_atom_field" in refusal.stderr,
            "Explicit valence was not refused",
        )
        defaults_path = root / "charged-radical-default-h.mol"
        defaults_path.write_text(_hydrogen_defaults(), encoding="utf-8")
        defaults = rdkit.Chem.MolFromMolFile(str(defaults_path), removeHs=False)
        _require(
            defaults is not None,
            "RDKit refused the charged/radical default-H fixture",
        )
        hydrogen_observations = [
            (
                a.GetSymbol(),
                a.GetFormalCharge(),
                a.GetNumRadicalElectrons(),
                a.GetNumImplicitHs(),
            )
            for a in defaults.GetAtoms()
        ]
        _require(
            hydrogen_observations[0][0:2] == ("N", 1)
            and hydrogen_observations[1][0:3] == ("C", 0, 1),
            "RDKit charge or radical differs",
        )
        _LOGGER.info(
            "RDKit charged/radical default-H observations: %s",
            hydrogen_observations,
        )
        # The selected local executable is called directly, without a shell.
        result = subprocess.run(  # noqa: S603
            [str(binary), "preview", str(defaults_path)],
            check=True,
            capture_output=True,
            text=True,
        )
        preview = json.loads(result.stdout)
        _require(
            preview["outcome"] == "partial",
            "Default-H import did not report uncertainty",
        )
        _require(
            any(
                item["feature"] == "default_hydrogen_interpretation"
                and item["disposition"] == "unknown"
                for item in preview["loss_report"]
            ),
            "Default-H uncertainty is missing",
        )
        _require(
            all(
                atom["hydrogens"]["mode"] == "inferred"
                for atom in preview["result"]["document"]["atoms"]
            ),
            "The recorded hydrogen state differs",
        )
        _check_coordinate_stereo(binary, root)
    _LOGGER.info(
        "RDKit graph, SDF, reciprocal reader and cis/trans checks passed"
    )


if __name__ == "__main__":
    main()
