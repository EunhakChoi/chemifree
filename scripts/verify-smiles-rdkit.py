"""Compare an independent RDKit 2022.03.2 with Chemifree's SMILES operations.

Usage:

    python -I scripts/verify-smiles-rdkit.py /path/to/chemifree

where the argument is the built CLI. Every fixture declares its expected
outcome: either the structure Chemifree reads and writes, or the specific
refusal code (and where it is raised). A fixture never passes because of an
unexpected error, and an expected refusal never passes because of a different
one.

For a fixture expected to be accepted the script checks that

* Chemifree's recorded and canonical output describe the same graph as the
  input, by RDKit's own canonical SMILES (a graph comparison made by the
  independent toolkit, not by Chemifree);
* Chemifree's canonical text is a fixpoint of Chemifree's own import and write;
* every other spelling of the same molecule that RDKit produces (random atom
  orders and the Kekule form) gives the identical Chemifree canonical text.

Chemifree's canonical form is its own and is not compared with RDKit's.
"""

from __future__ import annotations

from collections.abc import Iterator, Sequence
import dataclasses
import json
import pathlib
import random
import subprocess
import sys
import tempfile
from typing import Any

import rdkit
import rdkit.Chem
import rdkit.Chem.rdMolDescriptors
import rdkit.RDLogger

_RANDOM_SPELLINGS = 6
_SEED = 20260201


@dataclasses.dataclass(frozen=True)
class Fixture:
    """One input and the outcome it must have.

    Attributes:
        smiles: The text given to both toolkits.
        refusal: None when Chemifree must accept and write the structure;
            otherwise the diagnostic code it must refuse with.
        stage: Where a refusal happens: "read" for the import preview,
            "canonical" for the canonical write after a successful read.
        offset: The byte offset the refusal must report, when it has one.
        rdkit: Whether RDKit must "accept" or "reject" the text, or "any".
    """

    smiles: str
    refusal: str | None = None
    stage: str = "read"
    offset: int | None = None
    rdkit: str = "accept"


ACCEPTED = (
    "CCO",
    "OCC",
    "CC(C)(C)C",
    "C=CC=C",
    "C#N",
    "C1CC1",
    "C1CCCCC1",
    "C1CCC2CCCCC2C1",
    "C1CCC2(C1)CCCC2",
    "C1CC2CCC1C2",
    "C1=CC2CCC1C2",
    "CC1=CCC2CC1C2(C)C",
    "C1C2CC3CC1CC(C2)C3",
    "c1ccccc1",
    "C1=CC=CC=C1",
    "Cc1ccccc1",
    "Oc1ccccc1",
    "c1ccc(cc1)-c1ccccc1",
    "c1ccncc1",
    "c1cc[nH]c1",
    "Cn1cccc1",
    "c1ccoc1",
    "c1ccsc1",
    "c1c[nH]cn1",
    "c1cn[nH]c1",
    "c1ccc2ccccc2c1",
    "c1ccc2cc3ccccc3cc2c1",
    "C1=CC=CC=C[CH-]1",
    "c1cc[nH+]cc1",
    "C[n+]1ccccc1",
    "c1cc[cH-]c1",
    "[NH4+]",
    "C[N+](C)(C)C",
    "CC(=O)[O-]",
    "[2H]O[2H]",
    "[13CH3]O",
    "CC(=O)[O-].[NH4+]",
    "[Cl-].C[NH3+]",
    "CCO.c1ccccc1",
    "[H]C([H])([H])O[H]",
    "[H][H]",
    "C%10CCCCC%10",
    "C1CCC11CCCC1",
    "C1.C1",
)

REFUSED = (
    # Reader refusals: invalid text, with the byte they point at.
    Fixture("", "empty_input", offset=0, rdkit="any"),
    Fixture(" CC", "leading_whitespace", offset=0, rdkit="any"),
    Fixture("C=(O)", "dangling_bond", offset=1, rdkit="reject"),
    Fixture("C(", "unclosed_branch", offset=1, rdkit="reject"),
    Fixture("C)", "unmatched_close_branch", offset=1, rdkit="reject"),
    Fixture("C1CC", "unclosed_ring", offset=1, rdkit="reject"),
    Fixture("C(C)1", "ring_digit_after_branch_close", offset=4, rdkit="any"),
    Fixture("C11", "ring_closure_self", offset=2, rdkit="reject"),
    Fixture("[C", "unclosed_bracket_atom", offset=0, rdkit="reject"),
    Fixture("[C+9]", "charge_beyond_drawing_range", offset=2, rdkit="any"),
    Fixture("[C+16]", "charge_out_of_range", offset=2, rdkit="any"),
    Fixture("C%(12)C", "extended_ring_label_unsupported", offset=1, rdkit="any"),
    Fixture("c", "aromatic_atom_outside_ring", offset=0, rdkit="reject"),
    Fixture("c:c", "aromatic_bond_outside_ring", offset=1, rdkit="reject"),
    # Valid SMILES that Chemifree does not implement.
    Fixture("C[C@H](F)Cl", "chirality_unsupported", offset=3),
    Fixture("C/C=C/C", "directional_bond_unsupported", offset=1),
    Fixture("C*", "wildcard_unsupported", offset=1, rdkit="any"),
    Fixture("[CH4:1]", "atom_class_unsupported", offset=4),
    Fixture("CC>>CO", "reaction_unsupported", offset=2, rdkit="any"),
    Fixture("[0H]", "isotope_zero_unsupported", offset=1, rdkit="any"),
    # Aromatic systems the shared model does not support.
    Fixture("c1ccc2[nH]ccc2c1", "aromatic_system_unsupported"),
    Fixture("C1CCc2ccccc2C1", "aromatic_system_unsupported"),
    Fixture("O=c1cccc[nH]1", "aromatic_system_unsupported"),
    # Read, but with no canonical form under the graph identity model.
    Fixture("[Na+].[Cl-]", "canonical_unsupported", stage="canonical"),
    Fixture("O=C1CCCCC1", "canonical_unsupported", stage="canonical"),
    Fixture("C1CCC2=CC=CC=C2C1", "canonical_unsupported", stage="canonical"),
    Fixture(
        "C1=CC=CC2=CC=CC=C2C=C1", "canonical_unsupported", stage="canonical"
    ),
)


class ChemifreeSession:
    """A running `chemifree operations` stream over a temporary data dir."""

    def __init__(self, binary: pathlib.Path, data_dir: pathlib.Path) -> None:
        # The caller supplies the local executable; no shell is involved.
        self._process = subprocess.Popen(  # noqa: S603
            [str(binary), "--data-dir", str(data_dir), "operations"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        self._count = 0
        self._open: list[str] = []

    def call(self, operation: str, parameters: dict[str, Any]) -> Any:
        """Sends one operation request and returns its decoded reply."""
        self._count += 1
        request = {
            "contract": 1,
            "request_id": f"verify-{self._count}",
            "profile": "preserve-semantics",
            "operation": operation,
            "parameters": parameters,
        }
        stdin, stdout = self._process.stdin, self._process.stdout
        if stdin is None or stdout is None:
            raise RuntimeError("The Chemifree process has no pipes.")
        stdin.write(json.dumps(request) + "\n")
        stdin.flush()
        line = stdout.readline()
        if not line:
            raise RuntimeError(f"Chemifree closed its output at {operation}.")
        return json.loads(line)

    def release(self) -> None:
        """Closes every drawing opened since the last release."""
        while self._open:
            document_id = self._open.pop()
            shown = self.call("document.snapshot", {"document_id": document_id})
            revision = shown["result"]["document"]["revision"]
            reply = self.call(
                "document.close",
                {
                    "document_id": document_id,
                    "discard_unsaved": True,
                    "expected_revision": revision,
                },
            )
            _require(reply["outcome"] == "ok", f"close: {reply}")

    def close(self) -> None:
        """Ends the stream and waits for the process."""
        if self._process.stdin is not None:
            self._process.stdin.close()
        if self._process.wait() != 0:
            raise RuntimeError("The Chemifree process failed.")

    def preview(self, smiles: str) -> tuple[str, Any]:
        """Creates an empty drawing and previews importing `smiles` into it."""
        created = self.call("document.create", {"title": "Fixture"})
        _require(created["result"] is not None, f"create: {created}")
        document_id = created["result"]["document"]["id"]
        self._open.append(document_id)
        reply = self.call(
            "document.import",
            {
                "document_id": document_id,
                "expected_revision": 0,
                "format": "smiles",
                "source": smiles,
                "preview": True,
            },
        )
        return document_id, reply

    def import_smiles(self, smiles: str) -> str:
        """Imports `smiles` into a new drawing and returns its ID."""
        document_id, shown = self.preview(smiles)
        _require(
            shown["outcome"] in ("ok", "partial"),
            f"{smiles!r}: preview refused: {shown['diagnostics']}",
        )
        digest = shown["result"]["preview_digest"]
        made = self.call(
            "document.import",
            {
                "document_id": document_id,
                "expected_revision": 0,
                "format": "smiles",
                "source": smiles,
                "preview_digest": digest,
                "accept_loss": True,
            },
        )
        _require(made["document_revision"] == 1, f"{smiles!r}: not imported")
        return document_id

    def write(self, document_id: str, form: str) -> Any:
        """Writes the drawing as SMILES and returns the whole reply."""
        return self.call(
            "document.export",
            {"format": "smiles", "document_id": document_id, "form": form},
        )


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _graph_key(smiles: str) -> str:
    """RDKit's canonical SMILES without stereo, as a graph identity."""
    molecule = rdkit.Chem.MolFromSmiles(smiles)
    _require(molecule is not None, f"RDKit rejects {smiles!r}")
    formula = rdkit.Chem.rdMolDescriptors.CalcMolFormula(molecule)
    canonical = rdkit.Chem.MolToSmiles(molecule, isomericSmiles=False)
    return f"{formula} {canonical}"


def _other_spellings(smiles: str) -> Iterator[str]:
    """Random atom orders and the Kekule form of the same molecule."""
    molecule = rdkit.Chem.MolFromSmiles(smiles)
    _require(molecule is not None, f"RDKit rejects {smiles!r}")
    # Renumbering gives other atom orders; RDKit's own doRandom fails for
    # molecules with several fragments in 2022.03.2.
    generator = random.Random(_SEED)  # noqa: S311  Reproducible fixtures.
    for _ in range(_RANDOM_SPELLINGS):
        order = list(range(molecule.GetNumAtoms()))
        generator.shuffle(order)
        renumbered = rdkit.Chem.RenumberAtoms(molecule, order)
        yield rdkit.Chem.MolToSmiles(renumbered, canonical=False)
    kekule = rdkit.Chem.Mol(molecule)
    rdkit.Chem.Kekulize(kekule, clearAromaticFlags=True)
    yield rdkit.Chem.MolToSmiles(kekule, kekuleSmiles=True)


def _check_accepted(session: ChemifreeSession, smiles: str) -> str:
    """Checks one accepted fixture and returns its canonical text."""
    expected = _graph_key(smiles)
    document_id = session.import_smiles(smiles)
    texts: dict[str, str] = {}
    for form in ("recorded", "canonical"):
        reply = session.write(document_id, form)
        _require(
            reply["outcome"] in ("ok", "partial"),
            f"{smiles!r} {form}: refused: {reply['diagnostics']}",
        )
        texts[form] = reply["result"]["content"]
        _require(
            _graph_key(texts[form]) == expected,
            f"{smiles!r} {form}: {texts[form]!r} is another graph",
        )
    canonical = texts["canonical"]
    again = session.write(session.import_smiles(canonical), "canonical")
    _require(
        again["result"]["content"] == canonical,
        f"{smiles!r}: {canonical!r} is not a fixpoint",
    )
    for spelling in _other_spellings(smiles):
        other = session.write(session.import_smiles(spelling), "canonical")
        _require(
            other["outcome"] in ("ok", "partial"),
            f"{smiles!r}: spelling {spelling!r} refused: "
            f"{other['diagnostics']}",
        )
        _require(
            other["result"]["content"] == canonical,
            f"{smiles!r}: spelling {spelling!r} gives "
            f"{other['result']['content']!r}, not {canonical!r}",
        )
    return canonical


def _check_refused(session: ChemifreeSession, fixture: Fixture) -> None:
    """Checks that a fixture is refused with exactly its declared code."""
    molecule = rdkit.Chem.MolFromSmiles(fixture.smiles)
    if fixture.rdkit == "accept":
        _require(
            molecule is not None,
            f"RDKit should accept {fixture.smiles!r}",
        )
    if fixture.rdkit == "reject":
        _require(
            molecule is None,
            f"RDKit should reject {fixture.smiles!r}",
        )
    if fixture.stage == "read":
        _, reply = session.preview(fixture.smiles)
    else:
        document_id = session.import_smiles(fixture.smiles)
        reply = session.write(document_id, "canonical")
    _require(
        reply["outcome"] in ("error", "unsupported"),
        f"{fixture.smiles!r} was not refused: {reply['outcome']}",
    )
    problem = reply["diagnostics"][0]
    _require(
        problem["code"] == fixture.refusal,
        f"{fixture.smiles!r}: expected {fixture.refusal}, "
        f"got {problem['code']}",
    )
    if fixture.offset is not None:
        _require(
            problem["details"].get("offset") == fixture.offset,
            f"{fixture.smiles!r}: offset {problem['details']}",
        )


def main(argv: Sequence[str]) -> int:
    """Runs every fixture against the CLI named by `argv[1]`."""
    if len(argv) != 2 or rdkit.__version__ != "2022.03.2":
        print(  # noqa: T201
            "usage: python -I verify-smiles-rdkit.py <chemifree binary>; "
            f"needs RDKit 2022.03.2 (found {rdkit.__version__})",
            file=sys.stderr,
        )
        return 2
    rdkit.RDLogger.DisableLog("rdApp.*")  # Rejections are checked, not logged.
    fixtures = [Fixture(smiles) for smiles in ACCEPTED] + list(REFUSED)
    with tempfile.TemporaryDirectory() as directory:
        session = ChemifreeSession(
            pathlib.Path(argv[1]), pathlib.Path(directory)
        )
        try:
            for fixture in fixtures:
                if fixture.refusal is None:
                    _check_accepted(session, fixture.smiles)
                else:
                    _check_refused(session, fixture)
                session.release()
        finally:
            session.close()
    accepted = len(ACCEPTED)
    print(  # noqa: T201
        f"{accepted} accepted and {len(REFUSED)} refused fixtures agree."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
