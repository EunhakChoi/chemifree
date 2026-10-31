"""Compare independent RDKit 2022.03.2 and Chemifree V2000 interpretations.

Run after building the ctfile_reference example, passing its absolute path. Fixtures
are written into a temporary directory and no network or proprietary inputs are used.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tempfile

import rdkit
import rdkit.Chem


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _read_with_chemifree(binary: pathlib.Path, path: pathlib.Path) -> dict:
    output = subprocess.run(
        [str(binary), "read", str(path)],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return json.loads(output.stdout)


def _butene(last_y: int) -> str:
    # No wedge/hash flags: opposing coordinates alone distinguish cis/trans.
    return (
        "2-butene\n\n\n"
        "  4  3  0  0  0  0  0  0  0  0999 V2000\n"
        "   -1.0000    1.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "    1.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n"
        f"    2.0000{float(last_y):10.4f}    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n"
        "  1  2  1  0  0  0  0\n"
        "  2  3  2  0  0  0  0\n"
        "  3  4  1  0  0  0  0\n"
        "M  END\n"
    )


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify-v2000-rdkit.py /absolute/path/to/ctfile_reference")
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    print(f"RDKit version: {rdkit.__version__}")
    _require(rdkit.__version__ == "2022.03.2", "reference version changed")
    with tempfile.TemporaryDirectory(prefix="chemifree-rdkit-") as directory:
        root = pathlib.Path(directory)
        subprocess.run([str(binary), "write", str(root)], check=True)
        mol = rdkit.Chem.MolFromMolFile(str(root / "molecule.mol"), sanitize=False, removeHs=False)
        _require(mol is not None, "RDKit refused the Chemifree MOL")
        _require(
            [(a.GetSymbol(), a.GetIsotope(), a.GetFormalCharge()) for a in mol.GetAtoms()]
            == [("C", 13, 0), ("O", 0, -1)],
            "RDKit atom interpretation differs",
        )
        _require(
            [(b.GetBeginAtomIdx(), b.GetEndAtomIdx(), str(b.GetBondType())) for b in mol.GetBonds()]
            == [(0, 1, "SINGLE")],
            "RDKit bond interpretation differs",
        )
        records = list(rdkit.Chem.SDMolSupplier(str(root / "records.sdf"), sanitize=False, removeHs=False))
        _require(len(records) == 2 and all(item is not None for item in records), "RDKit did not read both SDF records")
        _require(records[1].GetProp("Label") == "independent reader", "SDF value changed")
        reference = rdkit.Chem.MolFromSmiles("[13CH3]CO")
        _require(reference is not None, "RDKit could not build the reference graph")
        reference_path = root / "rdkit.mol"
        reference_path.write_text(rdkit.Chem.MolToMolBlock(reference), encoding="utf-8")
        parsed = _read_with_chemifree(binary, reference_path)["records"][0]
        _require([(a["element"], a["isotope"], a["charge"]) for a in parsed["atoms"]] == [("C", 13, 0), ("C", None, 0), ("O", None, 0)], "Chemifree interpreted RDKit's atoms incorrectly")
        _require([(b["begin"], b["end"], b["order"]) for b in parsed["bonds"]] == [(0, 1, 1), (1, 2, 1)], "Chemifree interpreted RDKit's bonds incorrectly")
        # RDKit writes an explicit valence for this anion. The bounded codec must
        # refuse that semantics rather than silently interpreting it as default.
        explicit_valence = rdkit.Chem.MolFromSmiles("[13CH3][CH2][O-]")
        _require(explicit_valence is not None, "RDKit could not build the anion")
        valence_path = root / "rdkit-explicit-valence.mol"
        valence_path.write_text(rdkit.Chem.MolToMolBlock(explicit_valence), encoding="utf-8")
        refusal = subprocess.run([str(binary), "read", str(valence_path)], capture_output=True, text=True, check=False)
        _require(refusal.returncode != 0 and "unsupported_atom_field" in refusal.stderr, "Explicit valence was not refused")
        for last_y, expected in [(1, "STEREOZ"), (-1, "STEREOE")]:
            source = _butene(last_y)
            reference_mol = rdkit.Chem.MolFromMolBlock(source, removeHs=False)
            _require(reference_mol is not None, "RDKit refused 2-butene")
            observed = str(reference_mol.GetBondWithIdx(1).GetStereo())
            _require(observed == expected, f"coordinate stereo: expected {expected}, got {observed}")
            path = root / f"butene-{last_y}.mol"
            path.write_text(source, encoding="utf-8")
            result = _read_with_chemifree(binary, path)
            _require(any(item["feature"] == "coordinate_stereo" for item in result["losses"]), "coordinate stereo uncertainty was omitted")
    print("RDKit graph, SDF, reciprocal reader and cis/trans coordinate checks passed")


if __name__ == "__main__":
    main()
