"""Compare independent RDKit 2022.03.2 and Chemifree SMILES interpretations.

Run after building the smiles_reference example and pass its absolute path.
Every fixture states its expected outcome explicitly: either the exact
canonical text or the refusal reason. A fixture never passes silently on an
error. For accepted fixtures the script checks that:

* every spelling gives the stated canonical text (spelling agreement);
* RDKit reads the spelling, Chemifree's recorded text and Chemifree's
  canonical text as the same graph (RDKit's own canonical SMILES agree);
* the canonical text is a fixpoint when Chemifree reads it again.

The Chemifree canonical form is its own, so only the graph, never the text,
is compared with RDKit.
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence
import dataclasses
import json
import logging
import pathlib
import subprocess
import sys
import tempfile

import rdkit
import rdkit.Chem

_LOGGER = logging.getLogger(__name__)


@dataclasses.dataclass(frozen=True)
class Accepted:
    """A fixture whose spellings all have this canonical text."""

    canonical: str


@dataclasses.dataclass(frozen=True)
class Refused:
    """A fixture whose spellings are all refused with one of these reasons."""

    reasons: tuple[str, ...]


@dataclasses.dataclass(frozen=True)
class Fixture:
    """One molecule, its spellings and the explicit expected outcome."""

    name: str
    spellings: tuple[str, ...]
    expected: Accepted | Refused
    # RDKit cannot sanitize some radicals or odd valences; those fixtures
    # still check Chemifree's outcome.
    compare_rdkit: bool = True


FIXTURES: tuple[Fixture, ...] = (
    Fixture("ethanol", ("CCO", "OCC", "C(O)C"), Accepted("C(C)O")),
    Fixture(
        "acetic acid",
        ("CC(=O)O", "OC(C)=O", "C(=O)(C)O"),
        Accepted("C(C)(=O)O"),
    ),
    Fixture(
        "benzene",
        ("c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1"),
        Accepted("c1ccccc1"),
    ),
    Fixture(
        "toluene",
        ("Cc1ccccc1", "c1ccccc1C", "CC1=CC=CC=C1"),
        Accepted("c1(C)ccccc1"),
    ),
    Fixture(
        "biphenyl",
        ("c1ccccc1-c1ccccc1", "c1ccccc1c1ccccc1", "C1=CC=CC=C1C1=CC=CC=C1"),
        Accepted("c1(ccccc1)-c1ccccc1"),
    ),
    Fixture(
        "pyridine",
        ("n1ccccc1", "N1=CC=CC=C1", "C1=CC=NC=C1"),
        Accepted("c1ccncc1"),
    ),
    Fixture(
        "pyrrole",
        ("[nH]1cccc1", "N1C=CC=C1", "C1=CNC=C1"),
        Accepted("c1c[nH]cc1"),
    ),
    Fixture(
        "imidazole",
        ("[nH]1cncc1", "N1C=NC=C1", "c1c[nH]cn1"),
        Accepted("c1c[nH]cn1"),
    ),
    Fixture("furan", ("o1cccc1", "O1C=CC=C1"), Accepted("c1cocc1")),
    Fixture("thiophene", ("s1cccc1", "S1C=CC=C1"), Accepted("c1cscc1")),
    Fixture(
        "naphthalene",
        ("c1ccc2ccccc2c1", "C1=CC=C2C=CC=CC2=C1"),
        Accepted("c12ccccc1cccc2"),
    ),
    Fixture("cyclopropane", ("C1CC1", "[CH2]1[CH2][CH2]1"), Accepted("C1CC1")),
    Fixture(
        "gem-dimethylcyclopropane",
        ("CC1(C)CC1", "C1CC1(C)C"),
        Accepted("C1(C)(C)CC1"),
    ),
    Fixture(
        "spiro[4.5]decane",
        ("C1CCC2(CC1)CCCC2", "C1CCCC12CCCCC2"),
        Accepted("C12(CCCC1)CCCCC2"),
    ),
    Fixture(
        "norbornene",
        ("C1CC2C=CC1C2", "C1=CC2CCC1C2"),
        Accepted("C12C=CC(C1)CC2"),
    ),
    Fixture(
        "alpha-pinene",
        ("CC1=CCC2CC1C2(C)C",),
        Accepted("C1(C)(C)C2C(C)=CCC1C2"),
    ),
    Fixture(
        "ammonium acetate",
        ("CC(=O)[O-].[NH4+]", "[NH4+].[O-]C(C)=O"),
        Accepted("C(C)(=O)[O-].[NH4+]"),
    ),
    Fixture("labelled ethane", ("[13CH3]C", "C[13CH3]"), Accepted("C[13CH3]")),
    Fixture(
        "indane",
        ("c1ccc2c(c1)CCC2", "C1=CC=C2C(=C1)CCC2"),
        Refused(("fused_partially_aromatic",)),
    ),
    Fixture(
        "indole",
        ("c1ccc2[nH]ccc2c1", "C1=CC=C2NC=CC2=C1"),
        Refused(("fused_system_outside_model",)),
    ),
    Fixture(
        "cyclohexanone",
        ("O=C1CCCCC1", "C1CCC(=O)CC1"),
        Refused(("unsupported_external_bond",)),
    ),
    Fixture(
        "sodium chloride", ("[Na+].[Cl-]",), Refused(("element_or_charge",))
    ),
)

# Input, expected reason and byte span of the Chemifree syntax error.
READ_ERRORS: tuple[tuple[str, str, int, int], ...] = (
    ("", "empty_input", 0, 0),
    (" CCO", "leading_whitespace", 0, 1),
    ("C(C", "unclosed_branch", 1, 2),
    ("C1CC", "unclosed_ring", 1, 2),
    ("C(C)1CC1", "ring_closure_after_branch", 4, 5),
    ("C[Xx]", "unknown_element", 2, 3),
    ("Cé", "unexpected_character", 1, 3),
    ("F/C=C/F", "stereo_unsupported", 1, 2),
    ("C[C@H](F)Cl", "stereo_unsupported", 3, 4),
)


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def _run(
    binary: pathlib.Path, root: pathlib.Path, lines: Sequence[str]
) -> dict[str, Mapping[str, object]]:
    """Run the reference binary over lines and index its answers by input."""
    path = root / "input.smi"
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    # The caller explicitly supplies the local executable; no shell is used.
    output = subprocess.run(  # noqa: S603
        [str(binary), str(path)],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    answers: dict[str, Mapping[str, object]] = {}
    for line in output.stdout.splitlines():
        row = json.loads(line)  # a dynamic protocol row, checked below
        _require(isinstance(row, dict), "A reference row must be an object.")
        answers[str(row["input"])] = row
    _require(
        set(answers) == set(lines), "The reference did not answer every line."
    )
    return answers


def _graph_key(smiles: str) -> str:
    """RDKit's connectivity-only canonical SMILES for a reading."""
    mol = rdkit.Chem.MolFromSmiles(smiles)
    _require(mol is not None, f"RDKit refused {smiles!r}")
    return str(rdkit.Chem.MolToSmiles(mol, isomericSmiles=False))


def _outcome(row: Mapping[str, object], key: str) -> Mapping[str, object]:
    value = row.get(key)
    _require(isinstance(value, dict), f"{row['input']!r}: no {key} outcome")
    return value  # type: ignore[return-value]  # checked just above


def _check_fixture(
    fixture: Fixture, answers: Mapping[str, Mapping[str, object]]
) -> list[str]:
    """Check one fixture; returns canonical texts to re-read."""
    canonical_texts: list[str] = []
    for spelling in fixture.spellings:
        row = answers[spelling]
        _require("read_error" not in row, f"{fixture.name}: {spelling!r} {row}")
        canonical = _outcome(row, "canonical")
        recorded = _outcome(row, "recorded")
        if isinstance(fixture.expected, Refused):
            reason = canonical.get("refused")
            _require(
                reason in fixture.expected.reasons,
                f"{fixture.name}: {spelling!r} gave {canonical}, expected a "
                f"refusal from {fixture.expected.reasons}",
            )
            continue
        _require(
            canonical.get("smiles") == fixture.expected.canonical,
            f"{fixture.name}: {spelling!r} gave {canonical}, "
            f"expected {fixture.expected.canonical!r}",
        )
        canonical_texts.append(fixture.expected.canonical)
        if not fixture.compare_rdkit:
            continue
        reference = _graph_key(spelling)
        _require(
            _graph_key(fixture.expected.canonical) == reference,
            f"{fixture.name}: canonical text differs from RDKit's graph",
        )
        recorded_text = recorded.get("smiles")
        _require(
            isinstance(recorded_text, str)
            and _graph_key(recorded_text) == reference,
            f"{fixture.name}: recorded text {recorded} differs from RDKit's "
            "graph",
        )
    return canonical_texts


def main() -> None:
    """Run the fixture, read-error, fixpoint and RDKit comparisons."""
    logging.basicConfig(
        level=logging.INFO, format="%(message)s", stream=sys.stdout
    )
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: verify-smiles-rdkit.py /absolute/path/to/smiles_reference"
        )
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    _LOGGER.info("RDKit version: %s", rdkit.__version__)
    _require(rdkit.__version__ == "2022.03.2", "reference version changed")
    spellings = [s for f in FIXTURES for s in f.spellings]
    errors = [text for text, *_ in READ_ERRORS]
    with tempfile.TemporaryDirectory(prefix="chemifree-smiles-") as directory:
        root = pathlib.Path(directory)
        answers = _run(binary, root, spellings + errors)
        again: list[str] = []
        for fixture in FIXTURES:
            again.extend(_check_fixture(fixture, answers))
            _LOGGER.info("ok  %s", fixture.name)
        for text, reason, start, end in READ_ERRORS:
            error = answers[text].get("read_error")
            _require(
                error == {"reason": reason, "start": start, "end": end},
                f"{text!r}: expected {reason} at {start}..{end}, got {error}",
            )
            if reason in (
                "unknown_element",
                "unclosed_branch",
                "unclosed_ring",
            ):
                _require(
                    rdkit.Chem.MolFromSmiles(text) is None,
                    f"RDKit unexpectedly accepted {text!r}",
                )
        _LOGGER.info("ok  %d read errors with positions", len(READ_ERRORS))
        fixed = _run(binary, root, sorted(set(again)))
        for text in sorted(set(again)):
            _require(
                _outcome(fixed[text], "canonical").get("smiles") == text,
                f"canonical text {text!r} is not a fixpoint",
            )
        _LOGGER.info("ok  %d canonical fixpoints", len(set(again)))


if __name__ == "__main__":
    main()
