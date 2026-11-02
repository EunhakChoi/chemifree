"""Compare Chemifree substructure matching with an RDKit 2022.03.2 Python.

Run after building the CLI and pass its absolute path:

    python -I scripts/verify-query-rdkit.py /path/to/chemifree

Every fixture in crates/application/tests/data/query-fixtures.json declares its
expected outcome: either the exact set of atom mappings (the target atom for
each query atom, in query-atom order) or a specific refusal (code, side and
reason). For a fixture with mappings, RDKit's GetSubstructMatches(uniquify=False)
and Chemifree's `chem.substructure` must both give exactly that set. For a
refusal, Chemifree must give exactly that refusal, and the fixture states how
many mappings RDKit finds, so a refusal cannot hide a query that RDKit answers
as zero. A fixture that gives anything else, including an error, fails; nothing
passes silently.

To author a fixture, leave out `expect` (or `refused.rdkit_matches`) and run
with `--fill`: the script prints the fixture file with RDKit's answer filled
in, for review.

The query is built as an RDKit query molecule: each atom a SMARTS atom (a
stated charge or isotope is a condition, a hydrogen count is not), each bond
single, double, triple or aromatic as RDKit perceives the sanitized drawing,
or the listed kinds. The target is a sanitized RDKit molecule that keeps drawn
hydrogens as atoms. No network and no proprietary input is used.
"""

from __future__ import annotations

import argparse
import json
import logging
import pathlib
import subprocess
import sys
import tempfile
from typing import Any

import rdkit
import rdkit.Chem
import rdkit.RDLogger

_LOGGER = logging.getLogger(__name__)
# A fixture may be unsanitizable on purpose; the script reports what matters.
rdkit.RDLogger.DisableLog("rdApp.*")
_EXPECTED_RDKIT = "2022.03.2"
_FIXTURES = (
    pathlib.Path(__file__).resolve().parent.parent
    / "crates/application/tests/data/query-fixtures.json"
)
_WILDCARDS = {
    "*": "*",
    "A": "[!#1]",
    "Q": "[!#1;!#6]",
    "X": "[#9,#17,#35,#53,#85,#117]",
}
_BOND_TYPES = {
    "-": rdkit.Chem.BondType.SINGLE,
    "=": rdkit.Chem.BondType.DOUBLE,
    "#": rdkit.Chem.BondType.TRIPLE,
    ":": rdkit.Chem.BondType.AROMATIC,
}
_BOND_SMARTS = {"-": "-", "=": "=", "#": "#", ":": ":"}
_MAX_MATCHES = 100_000


class FixtureError(Exception):
    """A fixture the script cannot evaluate."""


def _parse_element(spec: str) -> tuple[int | None, str, int, int | None]:
    """Reads `13C`, `N+`, `O-`, `N@1`: isotope, symbol, charge, hydrogens."""
    hydrogens = None
    if "@" in spec:
        spec, count = spec.split("@")
        hydrogens = int(count)
    digits = len(spec) - len(spec.lstrip("0123456789"))
    isotope = int(spec[:digits]) if digits else None
    rest = spec[digits:]
    sign = next((i for i, c in enumerate(rest) if c in "+-"), len(rest))
    symbol = rest[:sign]
    charge = {"": 0, "+": 1, "-": -1}.get(rest[sign:])
    if charge is None:
        charge = int(rest[sign:])
    return isotope, symbol, charge, hydrogens


def _query_atom_smarts(spec: str) -> str:
    """The SMARTS atom for a query-side atom spec."""
    if spec in _WILDCARDS:
        return _WILDCARDS[spec]
    if spec.startswith(("![", "[")):
        symbols = spec.lstrip("!").strip("[]").split(",")
        numbers = [
            rdkit.Chem.GetPeriodicTable().GetAtomicNumber(s) for s in symbols
        ]
        if spec.startswith("!"):
            return "[" + ";".join(f"!#{n}" for n in numbers) + "]"
        return "[" + ",".join(f"#{n}" for n in numbers) + "]"
    isotope, symbol, charge, _ = _parse_element(spec)
    number = rdkit.Chem.GetPeriodicTable().GetAtomicNumber(symbol)
    text = f"{isotope or ''}#{number}"
    if charge:
        text += f";{charge:+d}"
    return f"[{text}]"


def _placeholder(atoms: list[str], bonds: list[list[Any]]) -> rdkit.Chem.Mol:
    """The query drawn as a molecule, to ask RDKit which bonds are aromatic.

    Wildcards become dummy atoms and query bonds single bonds; if the drawing
    cannot be sanitized the spelled bond types stand.
    """
    mol = rdkit.Chem.RWMol()
    for spec in atoms:
        if spec in _WILDCARDS or spec.lstrip("!").startswith("["):
            mol.AddAtom(rdkit.Chem.Atom(0))
        else:
            _, symbol, charge, hydrogens = _parse_element(spec)
            atom = rdkit.Chem.Atom(symbol)
            atom.SetFormalCharge(charge)
            if hydrogens is not None:
                atom.SetNumExplicitHs(hydrogens)
                atom.SetNoImplicit(True)
            mol.AddAtom(atom)
    for first, second, kind in bonds:
        bond_type = _BOND_TYPES.get(kind, rdkit.Chem.BondType.SINGLE)
        mol.AddBond(first, second, bond_type)
        if bond_type == rdkit.Chem.BondType.AROMATIC:
            mol.GetBondBetweenAtoms(first, second).SetIsAromatic(True)
            mol.GetAtomWithIdx(first).SetIsAromatic(True)
            mol.GetAtomWithIdx(second).SetIsAromatic(True)
    try:
        rdkit.Chem.SanitizeMol(mol)
    except ValueError:
        _LOGGER.debug("placeholder not sanitizable; spelled bonds stand")
    return mol


def build_query(spec: dict[str, Any]) -> rdkit.Chem.Mol:
    """The RDKit query molecule for a fixture's query."""
    atoms: list[str] = spec["atoms"]
    bonds: list[list[Any]] = spec["bonds"]
    placeholder = _placeholder(atoms, bonds)
    query = rdkit.Chem.RWMol()
    for text in atoms:
        query.AddAtom(rdkit.Chem.AtomFromSmarts(_query_atom_smarts(text)))
    for first, second, kind in bonds:
        query.AddBond(first, second, rdkit.Chem.BondType.SINGLE)
        index = query.GetBondBetweenAtoms(first, second).GetIdx()
        if kind == "~":
            smarts = "~"
        elif len(kind) > 1:
            smarts = ",".join(_BOND_SMARTS[c] for c in kind)
        elif placeholder.GetBondBetweenAtoms(first, second).GetIsAromatic():
            smarts = ":"
        else:
            smarts = _BOND_SMARTS[kind]
        query.ReplaceBond(index, rdkit.Chem.BondFromSmarts(smarts))
    return query


def build_target(spec: dict[str, Any]) -> rdkit.Chem.Mol:
    """A sanitized RDKit molecule; drawn hydrogens stay atoms."""
    mol = rdkit.Chem.RWMol()
    for text in spec["atoms"]:
        if text in _WILDCARDS or text.lstrip("!").startswith("["):
            raise FixtureError(f"RDKit cannot sanitize a query target: {text}")
        isotope, symbol, charge, hydrogens = _parse_element(text)
        atom = rdkit.Chem.Atom(symbol)
        atom.SetFormalCharge(charge)
        if isotope:
            atom.SetIsotope(isotope)
        if hydrogens is not None:
            atom.SetNumExplicitHs(hydrogens)
            atom.SetNoImplicit(True)
        mol.AddAtom(atom)
    for first, second, kind in spec["bonds"]:
        if kind not in _BOND_TYPES:
            raise FixtureError(f"RDKit target bond {kind!r} is not compared")
        mol.AddBond(first, second, _BOND_TYPES[kind])
        if kind == ":":
            mol.GetBondBetweenAtoms(first, second).SetIsAromatic(True)
            mol.GetAtomWithIdx(first).SetIsAromatic(True)
            mol.GetAtomWithIdx(second).SetIsAromatic(True)
    rdkit.Chem.SanitizeMol(mol)
    return mol


def rdkit_mappings(fixture: dict[str, Any]) -> set[tuple[int, ...]]:
    """Every mapping RDKit finds, as target indices in query-atom order."""
    query = build_query(fixture["query"])
    target = build_target(fixture["target"])
    matches = target.GetSubstructMatches(
        query, uniquify=False, maxMatches=_MAX_MATCHES
    )
    if len(matches) >= _MAX_MATCHES:
        raise FixtureError("RDKit hit its match limit")
    return {tuple(match) for match in matches}


def _commands(spec: dict[str, Any]) -> list[dict[str, Any]]:
    commands: list[dict[str, Any]] = []
    updates: list[dict[str, Any]] = []
    for index, text in enumerate(spec["atoms"]):
        add: dict[str, Any] = {
            "op": "add_atom",
            "position": {"x": 30 * index, "y": 0},
        }
        if text in ("*", "A", "Q", "X"):
            names = {"*": "any", "A": "any_heavy", "Q": "heteroatom", "X": "halogen"}
            add["query"] = {"kind": names[text]}
        elif text.lstrip("!").startswith("["):
            add["query"] = {
                "kind": "list",
                "elements": text.lstrip("!").strip("[]").split(","),
                "negated": text.startswith("!"),
            }
        else:
            isotope, symbol, charge, hydrogens = _parse_element(text)
            add["element"] = symbol
            if hydrogens is not None:
                add["hydrogens"] = {"mode": "specified", "count": hydrogens}
            change: dict[str, Any] = {}
            if charge:
                change["charge"] = charge
            if isotope:
                change["isotope"] = isotope
            if change:
                updates.append(
                    {"op": "update_atom", "atom": f"a{index + 1}", **change}
                )
        commands.append(add)
    orders = {"-": "single", "=": "double", "#": "triple", ":": "aromatic"}
    for first, second, kind in spec["bonds"]:
        bond: dict[str, Any] = {
            "op": "add_bond",
            "begin": f"${first}",
            "end": f"${second}",
        }
        if kind in orders:
            bond["order"] = orders[kind]
        elif kind == ">":
            bond["order"] = "dative"
        elif kind == "?":
            bond["order"] = "unknown"
        elif kind == "~":
            bond.update(order="query", query={"kind": "any"})
        else:
            bond.update(
                order="query",
                query={"kind": "list", "orders": [orders[c] for c in kind]},
            )
        commands.append(bond)
    count = len(spec["atoms"])
    return commands[:count] + updates + commands[count:]


def _request(identifier: str, operation: str, parameters: dict[str, Any]) -> str:
    return json.dumps(
        {
            "contract": 1,
            "request_id": identifier,
            "profile": "preserve-semantics",
            "operation": operation,
            "parameters": parameters,
        }
    )


def _pairs(mapping: dict[str, Any]) -> tuple[int, ...]:
    """A reply mapping as target indices in query-atom order."""
    atoms = sorted(
        ((int(p["query"][1:]), int(p["target"][1:])) for p in mapping["atoms"])
    )
    return tuple(target - 1 for _, target in atoms)


def check(cli: pathlib.Path, fixture: dict[str, Any], fill: bool) -> list[str]:
    """Returns the problems found in one fixture (empty when it passes)."""
    name = fixture["name"]
    problems: list[str] = []
    skipped = fixture.get("rdkit_skipped")
    rdkit_found = None if skipped else rdkit_mappings(fixture)
    reply = chemifree_reply(cli, fixture)
    refused = fixture.get("refused")
    if refused is None:
        if rdkit_found is None:
            problems.append(f"{name}: a fixture with mappings must be comparable with RDKit")
            return problems
        if fill and "expect" not in fixture:
            fixture["expect"] = {"mappings": sorted(rdkit_found)}
        expected = {tuple(m) for m in fixture["expect"]["mappings"]}
        if len(expected) != len(fixture["expect"]["mappings"]):
            problems.append(f"{name}: expected mappings repeat")
        if reply["outcome"] != "ok":
            problems.append(f"{name}: Chemifree answered {reply['outcome']}: {reply['diagnostics']}")
            return problems
        mappings = reply["result"]["mappings"]
        ours = {_pairs(m) for m in mappings}
        if len(ours) != len(mappings):
            problems.append(f"{name}: Chemifree repeated a mapping")
        if ours != expected:
            problems.append(f"{name}: Chemifree {sorted(ours)} != expected {sorted(expected)}")
        if rdkit_found != expected:
            problems.append(f"{name}: RDKit {sorted(rdkit_found)} != expected {sorted(expected)}")
        if reply["model_id"] != "chemifree-substructure-1":
            problems.append(f"{name}: model_id {reply['model_id']}")
        return problems
    if rdkit_found is None:
        return problems + _refusal_problems(name, reply, refused)
    if fill and "rdkit_matches" not in refused:
        refused["rdkit_matches"] = len(rdkit_found)
    problems.extend(_refusal_problems(name, reply, refused))
    if len(rdkit_found) != refused["rdkit_matches"]:
        problems.append(
            f"{name}: RDKit finds {len(rdkit_found)}, fixture says {refused['rdkit_matches']}"
        )
    return problems


def _refusal_problems(
    name: str, reply: dict[str, Any], refused: dict[str, Any]
) -> list[str]:
    """Problems with Chemifree's reply to a fixture that must be refused."""
    problems: list[str] = []
    diagnostic = reply["diagnostics"][0] if reply["diagnostics"] else {}
    got = (
        diagnostic.get("code"),
        diagnostic.get("details", {}).get("side"),
        diagnostic.get("details", {}).get("reason"),
    )
    want = (refused["code"], refused["side"], refused["reason"])
    if reply["result"] is not None or reply["outcome"] == "ok":
        problems.append(f"{name}: expected a refusal, got {reply['outcome']}")
    elif got != want:
        problems.append(f"{name}: refusal {got} != expected {want}")
    return problems


def chemifree_reply(cli: pathlib.Path, fixture: dict[str, Any]) -> dict[str, Any]:
    """Draws both graphs and asks `chem.substructure` in one CLI process.

    A CLI process is its own session, so drawing and matching share it. The
    CLI answers each line before reading the next, which is how the document
    IDs of the first replies reach the later requests. Atom IDs are `a1`,
    `a2`, ... in creation order, which is how the reply is read back as
    fixture indices.
    """
    with tempfile.TemporaryDirectory(prefix="chemifree-query-") as data:
        process = subprocess.Popen(
            [str(cli), "--data-dir", data, "operations"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        assert process.stdin is not None and process.stdout is not None

        def ask(line: str) -> dict[str, Any]:
            process.stdin.write(line + "\n")
            process.stdin.flush()
            return json.loads(process.stdout.readline())

        try:
            ids = [
                ask(_request(f"create-{n}", "document.create", {}))["result"][
                    "document"
                ]["id"]
                for n in ("query", "target")
            ]
            for name, identifier in zip(("query", "target"), ids):
                drawn = ask(
                    _request(
                        f"draw-{name}",
                        "document.transaction",
                        {
                            "document_id": identifier,
                            "expected_revision": 0,
                            "commands": _commands(fixture[name]),
                        },
                    )
                )
                if drawn["outcome"] != "ok":
                    raise FixtureError(f"{fixture['name']}: drawing failed: {drawn}")
            return ask(
                _request(
                    "match",
                    "chem.substructure",
                    {"query_document_id": ids[0], "target_document_id": ids[1]},
                )
            )
        finally:
            process.stdin.close()
            process.wait(timeout=60)


def main() -> int:
    """Checks every fixture; prints a summary and returns the exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("cli", type=pathlib.Path, help="path to the chemifree CLI")
    parser.add_argument("--fixtures", type=pathlib.Path, default=_FIXTURES)
    parser.add_argument("--fill", action="store_true", help="print the fixture file with RDKit's answers filled in")
    arguments = parser.parse_args()
    if rdkit.__version__ != _EXPECTED_RDKIT:
        print(f"expected RDKit {_EXPECTED_RDKIT}, found {rdkit.__version__}", file=sys.stderr)
        return 2
    document = json.loads(arguments.fixtures.read_text(encoding="utf-8"))
    names = [fixture["name"] for fixture in document["fixtures"]]
    if len(set(names)) != len(names):
        print("fixture names repeat", file=sys.stderr)
        return 1
    problems: list[str] = []
    answered = refused = 0
    for fixture in document["fixtures"]:
        problems.extend(check(arguments.cli, fixture, arguments.fill))
        if "refused" in fixture:
            refused += 1
        else:
            answered += 1
    if arguments.fill:
        print(json.dumps(document, indent=1))
    for problem in problems:
        print(problem, file=sys.stderr)
    print(
        f"{answered} answered and {refused} refused fixtures, "
        f"{len(problems)} problems",
        file=sys.stderr,
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
