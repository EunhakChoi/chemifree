"""Generate the aromaticity change-record corpus as MOL/V2000 records.

Each molecule is written in a Kekule spelling (no stated hydrogens) and, when
RDKit perceives aromatic bonds, an aromatic spelling in which aromatic
nitrogens with hydrogen keep that hydrogen as a stated count. The V2000 reader
accepts only single, double and triple bonds, so every molblock is Kekule and
the aromatic spelling lists the indexes of the bonds to record as aromatic.
Requires RDKit 2022.03.2:

    python -I scripts/aromaticity_corpus.py \
        docs/verification/aromaticity-corpus.json
"""

from __future__ import annotations

import json
import pathlib
import sys

from rdkit import Chem
from rdkit.Chem import AllChem

# (class, name, SMILES). Order is stable so the generated file is reproducible.
_MOLECULES: list[tuple[str, str, str]] = [
    ("acyclic", "ethane", "CC"),
    ("acyclic", "1,3-butadiene", "C=CC=C"),
    ("acyclic", "toluene side chain", "CCc1ccccc1"),
    ("saturated ring", "cyclopropane", "C1CC1"),
    ("saturated ring", "cyclohexane", "C1CCCCC1"),
    ("saturated ring", "cyclohexene", "C1=CCCCC1"),
    ("saturated ring", "1,3-cyclohexadiene", "C1=CC=CCC1"),
    ("saturated ring", "piperidine", "C1CCNCC1"),
    ("saturated ring", "tetrahydrofuran", "C1CCOC1"),
    ("saturated ring", "pyrrolidine", "C1CCNC1"),
    ("saturated ring", "morpholine", "C1COCCN1"),
    ("saturated ring", "cyclopentene", "C1=CCCC1"),
    ("saturated ring", "decalin", "C1CCC2CCCCC2C1"),
    ("benzene and monocycles", "benzene", "c1ccccc1"),
    ("benzene and monocycles", "toluene", "Cc1ccccc1"),
    ("benzene and monocycles", "phenol", "Oc1ccccc1"),
    ("benzene and monocycles", "aniline", "Nc1ccccc1"),
    ("benzene and monocycles", "biphenyl", "c1ccc(cc1)-c1ccccc1"),
    ("benzene and monocycles", "styrene", "C=Cc1ccccc1"),
    ("monocyclic heteroaromatic", "pyridine", "c1ccncc1"),
    ("monocyclic heteroaromatic", "pyrimidine", "c1cncnc1"),
    ("monocyclic heteroaromatic", "pyrazine", "c1cnccn1"),
    ("monocyclic heteroaromatic", "pyridazine", "c1ccnnc1"),
    ("monocyclic heteroaromatic", "1,3,5-triazine", "c1ncncn1"),
    ("monocyclic heteroaromatic", "pyrrole", "c1cc[nH]c1"),
    ("monocyclic heteroaromatic", "N-methylpyrrole", "Cn1cccc1"),
    ("monocyclic heteroaromatic", "furan", "c1ccoc1"),
    ("monocyclic heteroaromatic", "thiophene", "c1ccsc1"),
    ("monocyclic heteroaromatic", "imidazole", "c1c[nH]cn1"),
    ("monocyclic heteroaromatic", "N-methylimidazole", "Cn1ccnc1"),
    ("monocyclic heteroaromatic", "pyrazole", "c1cn[nH]c1"),
    ("monocyclic heteroaromatic", "oxazole", "c1cocn1"),
    ("monocyclic heteroaromatic", "isoxazole", "c1cnoc1"),
    ("monocyclic heteroaromatic", "thiazole", "c1cscn1"),
    ("monocyclic heteroaromatic", "1,2,4-triazole", "c1nc[nH]n1"),
    ("monocyclic heteroaromatic", "1,2,3-triazole", "c1c[nH]nn1"),
    ("monocyclic heteroaromatic", "tetrazole", "c1nnn[nH]1"),
    ("monocyclic heteroaromatic", "2-pyridone", "O=c1cccc[nH]1"),
    ("benzenoid", "naphthalene", "c1ccc2ccccc2c1"),
    ("benzenoid", "anthracene", "c1ccc2cc3ccccc3cc2c1"),
    ("benzenoid", "phenanthrene", "c1ccc2c(c1)ccc1ccccc12"),
    ("benzenoid", "pyrene", "c1cc2ccc3cccc4ccc(c1)c2c34"),
    ("benzenoid", "chrysene", "c1ccc2c(c1)ccc1c3ccccc3ccc21"),
    ("benzenoid", "tetracene", "c1ccc2cc3cc4ccccc4cc3cc2c1"),
    ("benzenoid", "triphenylene", "c1ccc2c(c1)c1ccccc1c1ccccc21"),
    ("benzenoid", "coronene", "c1cc2ccc3ccc4ccc5ccc6ccc1c1c2c3c4c5c61"),
    ("fused heteroaromatic", "indole", "c1ccc2[nH]ccc2c1"),
    ("fused heteroaromatic", "benzimidazole", "c1ccc2[nH]cnc2c1"),
    ("fused heteroaromatic", "benzofuran", "c1ccc2occc2c1"),
    ("fused heteroaromatic", "benzothiophene", "c1ccc2sccc2c1"),
    ("fused heteroaromatic", "carbazole", "c1ccc2c(c1)[nH]c1ccccc12"),
    ("fused heteroaromatic", "quinoline", "c1ccc2ncccc2c1"),
    ("fused heteroaromatic", "isoquinoline", "c1ccc2cnccc2c1"),
    ("fused heteroaromatic", "purine", "c1ncc2[nH]cnc2n1"),
    ("fused heteroaromatic", "benzoxazole", "c1ccc2ocnc2c1"),
    ("fused heteroaromatic", "indazole", "c1ccc2[nH]ncc2c1"),
    ("fused partially saturated", "tetralin", "C1CCc2ccccc2C1"),
    ("fused partially saturated", "indane", "C1Cc2ccccc2C1"),
    ("fused partially saturated", "indene", "C1=Cc2ccccc2C1"),
    ("fused partially saturated", "1,3-benzodioxole", "C1Oc2ccccc2O1"),
    ("fused partially saturated", "indoline", "C1Cc2ccccc2N1"),
    ("fused partially saturated", "isoindoline", "C1NCc2ccccc12"),
    ("fused partially saturated", "fluorene", "C1c2ccccc2-c2ccccc12"),
    (
        "fused partially saturated",
        "9,10-dihydroanthracene",
        "C1c2ccccc2Cc2ccccc12",
    ),
    ("fused partially saturated", "1,2-dihydronaphthalene", "C1=Cc2ccccc2CC1"),
    ("fused partially saturated", "chroman", "C1COc2ccccc2C1"),
    ("fused partially saturated", "tetrahydroquinoline", "C1CNc2ccccc2C1"),
    ("fused partially saturated", "dibenzofuran", "c1ccc2c(c1)oc1ccccc12"),
    ("fused partially saturated", "2,3-dihydrobenzofuran", "C1COc2ccccc12"),
    (
        "fused partially saturated",
        "estratriene skeleton",
        "C1CC2CCC3c4ccccc4CCC3C2C1",
    ),
    (
        "fused partially saturated",
        "benzene plus tetralin (separate)",
        "c1ccccc1.C1CCc2ccccc2C1",
    ),
    ("bridged", "norbornane", "C1CC2CCC1C2"),
    ("bridged", "norbornene", "C1=CC2CCC1C2"),
    ("bridged", "norbornadiene", "C1=CC2C=CC1C2"),
    ("bridged", "alpha-pinene", "CC1=CCC2CC1C2(C)C"),
    ("bridged", "adamantane", "C1C2CC3CC1CC(C2)C3"),
    ("bridged", "bicyclo[2.2.2]octadiene", "C1=CC2CCC1C=C2"),
    ("bridged", "barrelene", "C1=CC2C=CC1C=C2"),
    ("bridged", "7-oxanorbornene", "C1=CC2CCC1O2"),
    ("bridged", "camphor", "CC1(C)C2CCC1(C)C(=O)C2"),
    ("spiro", "spiro[4.4]nonane", "C1CCC2(C1)CCCC2"),
    ("spiro", "spiro[4.5]deca-6,9-diene", "C1CCC2(C1)C=CC=CC2"),
    ("spiro", "spirobenzene-cyclopentane", "C1CCC2(C1)c1ccccc1CC2"),
    ("spiro", "spirobifluorene", "c1ccc2c(c1)-c1ccccc1C21c2ccccc2-c2ccccc21"),
    ("charged", "cyclopentadienyl anion", "c1cc[cH-]c1"),
    ("charged", "tropylium", "c1ccc[cH+]cc1"),
    ("charged", "cyclopropenyl cation", "C1=C[CH+]1"),
    ("charged", "pyridinium", "c1cc[nH+]cc1"),
    ("charged", "N-methylpyridinium", "C[n+]1ccccc1"),
    ("charged", "imidazolium", "c1c[nH+]c[nH]1"),
    ("charged", "pyrrolide", "c1cc[n-]c1"),
    ("charged", "cycloheptatrienyl anion", "C1=CC=CC=C[CH-]1"),
    ("antiaromatic and large", "cyclobutadiene", "C1=CC=C1"),
    ("antiaromatic and large", "cyclooctatetraene", "C1=CC=CC=CC=C1"),
    ("antiaromatic and large", "cyclopropene", "C1=CC1"),
    (
        "antiaromatic and large",
        "[10]annulene (cyclodecapentaene)",
        "C1=CC=CC=CC=CC=C1",
    ),
    ("antiaromatic and large", "[18]annulene", "C1=CC=CC=CC=CC=CC=CC=CC=CC=C1"),
    ("azulene-like", "azulene", "c1ccc2cccc2cc1"),
    ("azulene-like", "heptalene", "C1=CC=CC2=CC=CC=C2C=C1"),
    ("azulene-like", "pentalene", "C1=CC2=CC=CC2=C1"),
    ("azulene-like", "biphenylene", "c1ccc2c(c1)-c1ccccc1-2"),
    ("azulene-like", "fulvene", "C=C1C=CC=C1"),
    ("donor-only and sigma rings", "cyclotriazane", "N1NN1"),
    ("donor-only and sigma rings", "1,2-dihydro-1,2-diazete", "N1NC=C1"),
    (
        "donor-only and sigma rings",
        "five-membered O-S ring (trioxadithiolane)",
        "O1SOSO1",
    ),
    ("donor-only and sigma rings", "triazane fused to CH2 ring", "C1CN2NN12"),
    ("donor-only and sigma rings", "1,2-dithiole", "S1SC=CC1"),
    ("radical", "cyclopentadienyl radical", "[CH]1C=CC=C1"),
    ("radical", "phenyl radical", "[c]1ccccc1"),
    ("radical", "cycloheptatrienylidene", "[C]1C=CC=CC=C1"),
    ("hypervalent", "N-dimethylpyrrole ylide (N valence 5)", "CN1(C)C=CC=C1"),
    (
        "hypervalent",
        "dimethylthiophene sulfurane (S valence 4)",
        "CS1(C)C=CC=C1",
    ),
    ("exocyclic double bond", "benzoquinone", "O=C1C=CC(=O)C=C1"),
    ("exocyclic double bond", "tropone", "O=C1C=CC=CC=C1"),
    ("exocyclic double bond", "cyclohexanone", "O=C1CCCCC1"),
    ("exocyclic double bond", "caffeine", "Cn1cnc2c1c(=O)n(C)c(=O)n2C"),
]


def _molblock(mol: Chem.Mol, kekulize: bool) -> str:
    """Return a V2000 block with deterministic 2D coordinates."""
    copy = Chem.Mol(mol)
    AllChem.Compute2DCoords(copy)
    lines = Chem.MolToMolBlock(copy, kekulize=kekulize).splitlines()
    atom_count = int(lines[3][0:3])
    # RDKit writes an explicit valence for charged atoms; the basic profile of
    # the reader requires the default (zero) there, and charges stay in M  CHG.
    for index in range(4, 4 + atom_count):
        lines[index] = lines[index][:48] + "  0" + lines[index][51:]
    return "\n".join(lines) + "\n"


def _parse(group: str, name: str, smiles: str) -> Chem.Mol:
    """Parse a SMILES; hypervalent records are kept unsanitized on purpose."""
    if group == "hypervalent":
        mol = Chem.MolFromSmiles(smiles, sanitize=False)
        if mol is not None:
            mol.UpdatePropertyCache(strict=False)
    else:
        mol = Chem.MolFromSmiles(smiles)
    if mol is None:
        raise ValueError(f"RDKit cannot parse {name}: {smiles}")
    return mol


def _entries(group: str, name: str, smiles: str) -> list[dict[str, object]]:
    mol = _parse(group, name, smiles)
    kekule = Chem.Mol(mol)
    Chem.Kekulize(kekule, clearAromaticFlags=True)
    entries: list[dict[str, object]] = [
        {
            "name": name,
            "class": group,
            "spelling": "kekule",
            "smiles": smiles,
            "stated_hydrogens": {},
            "aromatic_bonds": [],
            "molblock": _molblock(kekule, kekulize=True),
        }
    ]
    if any(bond.GetIsAromatic() for bond in mol.GetBonds()):
        stated = {
            str(atom.GetIdx()): atom.GetTotalNumHs()
            for atom in mol.GetAtoms()
            if atom.GetIsAromatic()
            and atom.GetSymbol() == "N"
            and atom.GetTotalNumHs() > 0
        }
        entries.append(
            {
                "name": name,
                "class": group,
                "spelling": "aromatic",
                "smiles": smiles,
                "stated_hydrogens": stated,
                "aromatic_bonds": [
                    bond.GetIdx()
                    for bond in mol.GetBonds()
                    if bond.GetIsAromatic()
                ],
                "molblock": _molblock(kekule, kekulize=True),
            }
        )
    return entries


def main(argv: list[str]) -> None:
    """Write the corpus JSON named by the single argument."""
    if len(argv) != 2:
        raise SystemExit(f"usage: {argv[0]} <output.json>")
    corpus = [
        entry
        for group, name, smiles in _MOLECULES
        for entry in _entries(group, name, smiles)
    ]
    pathlib.Path(argv[1]).write_text(
        json.dumps(corpus, indent=1, sort_keys=True) + "\n", encoding="utf-8"
    )
    sys.stdout.write(f"wrote {len(corpus)} records\n")


if __name__ == "__main__":
    main(sys.argv)
