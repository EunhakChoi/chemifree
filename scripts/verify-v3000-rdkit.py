"""Compare independent RDKit 2022.03.2 and the Chemifree V3000 codec core.

Hand-run, like verify-v2000-rdkit.py: build the ctfile_reference example and pass its
absolute path. Compared: atoms, charges, isotopes, radicals and bond types of what Chemifree
writes (read by RDKit) and of what RDKit writes (read by Chemifree), plus a continued line.
Bond marks (P28b1): the service-written wedge, hash and either marks, in V2000 and V3000, are
read by RDKit (the chirality the file gives, which shows the narrow end, and the crossed double bond);
and RDKit-written V3000 wedge marks are read by Chemifree (bond kind and begin atom).
Not compared: collection numbers (RDKit does not preserve zero or negative ones), query
features, SGroups and stereo groups, which are refused or not mapped yet; the CIP label RDKit
derives for the wedge fixture is checked against a hand derivation, not against Chemifree.
"""

from __future__ import annotations

import json
import logging
import pathlib
import subprocess
import sys
import tempfile

import rdkit
import rdkit.Chem
import rdkit.Chem.rdDepictor

_LOGGER = logging.getLogger(__name__)
_FIXTURES = (
    pathlib.Path(__file__).resolve().parent.parent
    / "crates/providers/tests/data/ctfile-v3000"
)


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _marks(binary: pathlib.Path, root: pathlib.Path) -> None:
    """Service-written marks read by RDKit, and RDKit-written marks read by Chemifree."""
    subprocess.run(  # noqa: S603
        [str(binary), "write-marks", str(root)], check=True
    )
    for dialect in ("v2000", "v3000"):
        # RDKit applies a wedge while it reads and then clears the bond direction, so the
        # chirality it assigns is the observable. Hand derivation for both drawings: H is
        # behind the plane, Br (lower left) to Cl (lower right) to F (top) runs
        # counterclockwise seen from the front, so S. In `center-end` the wedge bond is
        # written with the center first, so the narrow end is the center and the stereo shows.
        for name, center_index in (("center", 0), ("center-end", 1)):
            center = rdkit.Chem.MolFromMolFile(
                str(root / f"{name}-{dialect}.mol"), sanitize=True, removeHs=False
            )
            _require(center is not None, f"RDKit refused {name}-{dialect}")
            labels = rdkit.Chem.FindMolChiralCenters(
                center, useLegacyImplementation=False
            )
            _require(
                labels == [(center_index, "S")],
                f"{name}-{dialect}: chirality differs: {labels}",
            )
        chain = rdkit.Chem.MolFromMolFile(
            str(root / f"chain-{dialect}.mol"), sanitize=False, removeHs=False
        )
        _require(chain is not None, f"RDKit refused chain-{dialect}")
        bonds = {
            (b.GetBeginAtomIdx(), b.GetEndAtomIdx()): (
                str(b.GetBondDir()),
                str(b.GetStereo()),
            )
            for b in chain.GetBonds()
        }
        _require(len(bonds) == 3, f"{dialect}: chain bonds differ: {bonds}")
        # RDKit keeps no direction for the hash and the single either bond after reading
        # (they bear on no center here); only the crossed double bond is observable.
        _require(
            bonds[(2, 3)][1] == "STEREOANY",
            f"{dialect}: crossed double bond differs: {bonds}",
        )
    reverse = rdkit.Chem.MolFromSmiles("N[C@@H](C)C(=O)O")
    rdkit.Chem.rdDepictor.Compute2DCoords(reverse)
    rdkit.Chem.WedgeMolBonds(reverse, reverse.GetConformer())
    expected = sorted(
        (b.GetBeginAtomIdx(), b.GetEndAtomIdx(), str(b.GetBondDir()))
        for b in reverse.GetBonds()
        if b.GetBondDir() in (rdkit.Chem.BondDir.BEGINWEDGE, rdkit.Chem.BondDir.BEGINDASH)
    )
    _require(len(expected) == 1, f"RDKit wedged {len(expected)} bonds")
    path = root / "rdkit-wedge.mol"
    path.write_text(rdkit.Chem.MolToV3KMolBlock(reverse), encoding="utf-8")
    output = subprocess.run(  # noqa: S603
        [str(binary), "read-v3000", str(path)],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    record = json.loads(output.stdout)["records"][0]
    kinds = {"wedge": "BEGINWEDGE", "hash": "BEGINDASH"}
    parsed = sorted(
        (b["begin"], b["end"], kinds[b["stereo"]])
        for b in record["bonds"]
        if b["stereo"] in kinds
    )
    _require(parsed == expected, f"Chemifree read RDKit's marks as {parsed}")


def main() -> None:
    """Run the reciprocal checks."""
    logging.basicConfig(
        level=logging.INFO, format="%(message)s", stream=sys.stdout
    )
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: verify-v3000-rdkit.py /absolute/path/to/ctfile_reference"
        )
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    _require(rdkit.__version__ == "2022.03.2", "reference version changed")
    with tempfile.TemporaryDirectory(prefix="chemifree-rdkit-") as directory:
        root = pathlib.Path(directory)
        subprocess.run(  # noqa: S603
            [str(binary), "write-v3000", str(root)], check=True
        )
        mol = rdkit.Chem.MolFromMolFile(
            str(root / "molecule-v3000.mol"), sanitize=False, removeHs=False
        )
        _require(mol is not None, "RDKit refused the Chemifree V3000 MOL")
        atoms = [
            (a.GetSymbol(), a.GetIsotope(), a.GetFormalCharge(), a.GetNumRadicalElectrons())
            for a in mol.GetAtoms()
        ]
        _require(
            atoms == [("C", 13, 0, 0), ("O", 0, -1, 1), ("N", 15, 1, 0)],
            f"RDKit atom interpretation differs: {atoms}",
        )
        bonds = [str(b.GetBondType()) for b in mol.GetBonds()]
        _require(bonds == ["SINGLE", "AROMATIC"], f"bonds differ: {bonds}")
        conformer = mol.GetConformer()
        _require(
            conformer.GetAtomPosition(0).x == 1.0e15
            and conformer.GetAtomPosition(2).z == -1.0e15,
            "the continued atom line was read differently",
        )
        for name, count in [("core.mol", 5), ("continued.mol", 3)]:
            fixture = rdkit.Chem.MolFromMolFile(
                str(_FIXTURES / name), sanitize=False, removeHs=False
            )
            _require(
                fixture is not None and fixture.GetNumAtoms() == count,
                f"RDKit did not read {name}",
            )
        _marks(binary, root)
        source = rdkit.Chem.MolFromSmiles("CCC(=O)O")
        _require(source is not None, "RDKit could not build the reference")
        source.GetAtomWithIdx(0).SetIsotope(13)
        source.GetAtomWithIdx(4).SetFormalCharge(-1)
        rdkit.Chem.SanitizeMol(source)
        path = root / "rdkit-v3000.mol"
        path.write_text(rdkit.Chem.MolToV3KMolBlock(source), encoding="utf-8")
        output = subprocess.run(  # noqa: S603
            [str(binary), "read-v3000", str(path)],
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        )
        record = json.loads(output.stdout)["records"][0]
        parsed = [(a["element"], a["isotope"], a["charge"]) for a in record["atoms"]]
        _require(
            parsed
            == [("C", 13, 0), ("C", None, 0), ("C", None, 0), ("O", None, 0), ("O", None, -1)],
            f"Chemifree interpreted RDKit's atoms incorrectly: {parsed}",
        )
        orders = [(b["begin"], b["end"], b["order"]) for b in record["bonds"]]
        _require(
            orders == [(0, 1, 1), (1, 2, 1), (2, 3, 2), (2, 4, 1)],
            f"Chemifree interpreted RDKit's bonds incorrectly: {orders}",
        )
        # RDKit states VAL for an explicit-hydrogen bracket atom; the codec core refuses it.
        bracket = rdkit.Chem.MolFromSmiles("[13CH3]CC(=O)[O-]")
        _require(bracket is not None, "RDKit could not build the bracket form")
        valence_path = root / "rdkit-valence.mol"
        valence_path.write_text(
            rdkit.Chem.MolToV3KMolBlock(bracket), encoding="utf-8"
        )
        refusal = subprocess.run(  # noqa: S603
            [str(binary), "read-v3000", str(valence_path)],
            capture_output=True,
            text=True,
            check=False,
        )
        _require(
            refusal.returncode != 0
            and "v3000_unsupported_atom_property" in refusal.stderr,
            "VAL was not refused",
        )
    _LOGGER.info("RDKit and Chemifree agree on the V3000 core subset")


if __name__ == "__main__":
    main()
