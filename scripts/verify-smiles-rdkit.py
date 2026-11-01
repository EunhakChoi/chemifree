"""Compare Chemifree SMILES results with an independent RDKit 2022.03.2.

Run with an RDKit 2022.03.2 Python and the path of a built `chemifree`
executable:

    python -I scripts/verify-smiles-rdkit.py /path/to/chemifree

Every fixture declares its expected outcome up front: the reader either
accepts the text or refuses it with a named code, and the canonical writer
either gives text or refuses with a named code. An outcome that differs from
the declaration fails, so no fixture can pass by erroring silently. For
accepted fixtures the graphs are compared with RDKit's own canonical SMILES
(constitution only: stereo is not implemented), and Chemifree's canonical
text is checked for the fixpoint and for agreement across spellings. No
network or proprietary input is used.
"""

from __future__ import annotations

from collections.abc import Sequence
import dataclasses
import json
import pathlib
import subprocess
import sys
import tempfile

from rdkit import Chem
from rdkit import RDLogger


@dataclasses.dataclass(frozen=True)
class Fixture:
    """One molecule, its spellings and what Chemifree must do with them.

    Attributes:
        name: Human-readable name used in failure messages.
        spellings: SMILES texts that mean the same graph.
        read: "ok", or the code the reader must refuse every spelling with.
        canonical: "ok" (one shared text), or the code every spelling's
            canonical write must be refused with.
        rdkit_agrees: Whether RDKit's graph must equal Chemifree's recorded
            output. False for fixtures where the two toolkits legitimately
            differ, with the reason in `note`.
        note: Why an unusual expectation is right.
    """

    name: str
    spellings: Sequence[str]
    read: str = "ok"
    canonical: str = "ok"
    rdkit_agrees: bool = True
    note: str = ""


FIXTURES: Sequence[Fixture] = (
    Fixture("ethanol", ["CCO", "OCC", "C(O)C"]),
    Fixture("acetic acid", ["CC(=O)O", "OC(C)=O", "O=C(C)O"]),
    Fixture("neopentane", ["CC(C)(C)C", "C(C)(C)(C)C"]),
    Fixture("cyclohexane", ["C1CCCCC1", "C%10CCCCC%10"]),
    Fixture("spiro[4.5]decane", ["C1CCC2(C1)CCCCC2", "C12(CCCC1)CCCCC2"]),
    Fixture("norbornene", ["C1=CC2CCC1C2", "C1C2CCC1C=C2"]),
    Fixture("alpha-pinene", ["CC1=CCC2CC1C2(C)C", "C2(C)(C)C1CC=C(C)C2C1"]),
    Fixture("benzene", ["c1ccccc1", "C1=CC=CC=C1"]),
    Fixture("pyridine", ["c1ccncc1", "C1=CC=NC=C1"]),
    Fixture("pyrrole", ["c1cc[nH]c1", "C1=CNC=C1"]),
    Fixture("furan", ["c1ccoc1", "C1=COC=C1"]),
    Fixture("toluene", ["Cc1ccccc1", "CC1=CC=CC=C1"]),
    Fixture("biphenyl", ["c1ccccc1-c1ccccc1", "c1ccccc1c1ccccc1"]),
    Fixture("naphthalene", ["c1ccc2ccccc2c1", "C1=CC=C2C=CC=CC2=C1"]),
    Fixture("ammonium", ["[NH4+]"]),
    Fixture("acetate", ["CC(=O)[O-]", "[O-]C(C)=O"]),
    Fixture("deuterochloroform", ["[2H]C(Cl)(Cl)Cl", "ClC(Cl)(Cl)[2H]"]),
    Fixture("ethanol and water", ["CCO.O", "O.OCC"]),
    Fixture("methyl radical", ["[CH3]"]),
    Fixture(
        "sodium chloride",
        ["[Na+].[Cl-]"],
        canonical="canonical_unsupported",
        note="Na+ is outside the shared valence model.",
    ),
    Fixture(
        "carbene",
        ["[CH2]"],
        canonical="canonical_unsupported",
        note="Singlet and triplet fit equally; the radical stays unknown.",
    ),
    Fixture(
        "tetralin (aromatic spelling)",
        ["C1CCc2ccccc2C1"],
        read="aromatic_system_unsupported",
        canonical="unreadable",
        note="Model 2 refuses fused systems with saturated atoms.",
    ),
    Fixture(
        "tetralin (Kekule spelling)",
        ["C1CCC2=CC=CC=C2C1"],
        canonical="canonical_unsupported",
    ),
    Fixture(
        "indole (aromatic spelling)",
        ["c1ccc2[nH]ccc2c1"],
        read="aromatic_system_unsupported",
        canonical="unreadable",
        note="Model 2 reads fused carbocycles only.",
    ),
    Fixture(
        "stereo",
        ["C[C@H](O)N"],
        read="stereo_unsupported",
        canonical="unreadable",
    ),
    Fixture(
        "double-bond stereo",
        ["F/C=C/F"],
        read="stereo_unsupported",
        canonical="unreadable",
    ),
    Fixture(
        "dangling bond", ["C=(O)"], read="dangling_bond", canonical="unreadable"
    ),
    Fixture(
        "unclosed ring", ["C1CC"], read="unclosed_ring", canonical="unreadable"
    ),
    Fixture(
        "aromatic bridge",
        ["c1ccccc1:c1ccccc1"],
        read="aromatic_bridge_unsupported",
        canonical="unreadable",
        rdkit_agrees=False,
        note="RDKit accepts an aromatic bond between rings; Chemifree refuses.",
    ),
)


class FixtureError(AssertionError):
    """A fixture did not behave as declared."""


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise FixtureError(message)


def _mapping(value: object, what: str) -> dict[str, object]:
    """Narrows a decoded JSON value to an object, or fails naming `what`."""
    if not isinstance(value, dict):
        raise FixtureError(f"{what} is not an object: {value!r}")
    return {str(key): item for key, item in value.items()}


def _text(value: object, what: str) -> str:
    """Narrows a decoded JSON value to a string, or fails naming `what`."""
    if not isinstance(value, str):
        raise FixtureError(f"{what} is not a string: {value!r}")
    return value


class Session:
    """One `chemifree operations` process, spoken to in JSON lines."""

    def __init__(self, binary: pathlib.Path, data_dir: pathlib.Path) -> None:
        # The caller names the local executable; no shell is involved.
        self._process = subprocess.Popen(  # noqa: S603
            [str(binary), "--data-dir", str(data_dir), "operations"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        self._count = 0

    def call(
        self, operation: str, parameters: dict[str, object]
    ) -> dict[str, object]:
        """Sends one request and returns its reply."""
        self._count += 1
        request = {
            "contract": 1,
            "request_id": f"rdkit-{self._count}",
            "profile": "preserve-semantics",
            "operation": operation,
            "parameters": parameters,
        }
        stdin, stdout = self._process.stdin, self._process.stdout
        if stdin is None or stdout is None:
            raise FixtureError("the process pipes are closed")
        stdin.write(json.dumps(request) + "\n")
        stdin.flush()
        return _mapping(json.loads(stdout.readline()), "the reply")

    def close(self) -> None:
        """Ends the process."""
        if self._process.stdin is not None:
            self._process.stdin.close()
        self._process.wait(timeout=60)


def _first_diagnostic(reply: dict[str, object]) -> dict[str, object]:
    diagnostics = reply.get("diagnostics")
    if not isinstance(diagnostics, list) or not diagnostics:
        raise FixtureError(f"no diagnostic in {reply}")
    return _mapping(diagnostics[0], "the first diagnostic")


def _code(reply: dict[str, object]) -> str:
    return _text(_first_diagnostic(reply).get("code"), "the code")


def _result(reply: dict[str, object]) -> dict[str, object]:
    return _mapping(reply.get("result"), f"the result of {reply}")


def _import(session: Session, smiles: str) -> tuple[str, str | None]:
    """Imports into a new document; returns (id, None) or ("", code)."""
    created = _result(session.call("document.create", {}))
    document_id = _text(_mapping(created["document"], "document")["id"], "id")
    parameters: dict[str, object] = {
        "format": "smiles",
        "document_id": document_id,
        "expected_revision": 0,
        "source": smiles,
    }
    preview = session.call("document.import", {**parameters, "preview": True})
    if preview.get("outcome") not in ("ok", "partial"):
        return "", _code(preview)
    digest = _result(preview)["preview_digest"]
    committed = session.call(
        "document.import",
        {**parameters, "preview_digest": digest, "accept_loss": True},
    )
    _require(
        committed.get("outcome") in ("ok", "partial"),
        f"commit of {smiles!r} failed: {committed}",
    )
    return document_id, None


def _write(
    session: Session, document_id: str, style: str
) -> tuple[str | None, str | None]:
    """Writes accepting every consent; returns (text, None) or (None, code)."""
    parameters: dict[str, object] = {
        "format": "smiles",
        "document_id": document_id,
        "style": style,
    }
    reply = session.call("document.export", parameters)
    if (
        reply.get("outcome") == "error"
        and _code(reply) == "loss_consent_required"
    ):
        details = _mapping(_first_diagnostic(reply)["details"], "details")
        accepted = {**parameters, "accepted_losses": details["loss_digest"]}
        reply = session.call("document.export", accepted)
    if reply.get("outcome") in ("ok", "partial"):
        return _text(_result(reply)["content"], "content"), None
    return None, _code(reply)


def _rdkit_key(smiles: str) -> str:
    """RDKit's canonical constitution of `smiles`, or a failure."""
    molecule = Chem.MolFromSmiles(smiles)
    if molecule is None:
        raise FixtureError(f"RDKit cannot read {smiles!r}")
    return str(Chem.MolToSmiles(molecule, isomericSmiles=False))


def check(session: Session, fixture: Fixture) -> int:
    """Checks one fixture; returns the number of assertions made."""
    checks = 0
    canonical_texts: set[str] = set()
    name = fixture.name
    for spelling in fixture.spellings:
        document_id, refused = _import(session, spelling)
        expected = None if fixture.read == "ok" else fixture.read
        _require(
            refused == expected,
            f"{name} {spelling!r}: read gave {refused!r}, want {expected!r}",
        )
        checks += 1
        if refused is not None:
            _require(
                fixture.canonical == "unreadable",
                f"{name}: an unreadable spelling needs canonical='unreadable'",
            )
            continue
        recorded, recorded_code = _write(session, document_id, "recorded")
        if recorded is None:
            raise FixtureError(
                f"{name} {spelling!r}: recorded gave {recorded_code}"
            )
        if fixture.rdkit_agrees:
            _require(
                _rdkit_key(recorded) == _rdkit_key(spelling),
                f"{name} {spelling!r}: RDKit sees {recorded!r} differently",
            )
        checks += 1
        text, code = _write(session, document_id, "canonical")
        if fixture.canonical != "ok":
            _require(
                text is None and code == fixture.canonical,
                f"{name} {spelling!r}: canonical gave {text!r}/{code!r}, "
                f"want {fixture.canonical!r}",
            )
            checks += 1
            continue
        if text is None:
            raise FixtureError(f"{name} {spelling!r}: canonical refused {code}")
        canonical_texts.add(text)
        if fixture.rdkit_agrees:
            _require(
                _rdkit_key(text) == _rdkit_key(spelling),
                f"{name} {spelling!r}: RDKit sees canonical {text!r} "
                "differently",
            )
        again_id, again_refused = _import(session, text)
        _require(again_refused is None, f"{name}: {text!r} is not read back")
        again, again_code = _write(session, again_id, "canonical")
        _require(
            again == text,
            f"{name}: {text!r} is not a fixpoint ({again!r}, {again_code})",
        )
        checks += 3
    if fixture.canonical == "ok":
        _require(
            len(canonical_texts) == 1,
            f"{name}: spellings disagree: {sorted(canonical_texts)}",
        )
        checks += 1
    return checks


def main(argv: Sequence[str]) -> int:
    """Runs every fixture against the executable named in `argv`."""
    if len(argv) != 2:
        sys.stderr.write(__doc__ or "")
        return 2
    version = Chem.rdBase.rdkitVersion
    _require(version == "2022.03.2", f"RDKit {version}, need 2022.03.2")
    RDLogger.DisableLog("rdApp.*")
    binary = pathlib.Path(argv[1]).resolve()
    total = 0
    with tempfile.TemporaryDirectory(prefix="chemifree-smiles-") as work:
        for fixture in FIXTURES:
            # A fresh process per fixture keeps within the open-document limit.
            session = Session(binary, pathlib.Path(work))
            try:
                total += check(session, fixture)
            finally:
                session.close()
    sys.stdout.write(f"{len(FIXTURES)} fixtures, {total} checks passed\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
