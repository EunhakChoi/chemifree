"""Compare Chemifree substructure matching with RDKit 2022.03.2.

Usage: verify-query-rdkit.py /absolute/path/to/query_reference

Build the example first (`cargo build -p chemifree-application --example
query_reference`). Targets and queries are written once as SMILES and their
RDKit graphs are read back to build the Chemifree request, so both programs
see the same atoms and bonds. Every fixture declares its expected outcome:
either an exact mapping set (checked against RDKit as well) or a refusal code.

Intentional differences, so only comparable behavior is compared:

* Hydrogens: neither side matches implicit hydrogens; every fixture is a
  heavy-atom graph and RDKit queries never name hydrogens.
* Charge: a Chemifree element query atom must equal the target charge, so the
  SMARTS spell ``+0`` explicitly; a bare SMARTS element would ignore charge.
* Bonds: SMARTS use explicit bond primitives; the SMARTS default
  (single-or-aromatic) has no Chemifree counterpart.
* Aromaticity: ``recorded`` fixtures read the SMILES with sanitization off in
  RDKit, keeping the written bond types, like Chemifree's recorded mode.
  ``aromatic_target`` fixtures sanitize in RDKit and use Chemifree's
  ``chemifree-aromaticity-1`` model; they use rings where both models agree.
  Cyclobutadiene is the documented divergence: RDKit leaves it non-aromatic
  while Chemifree refuses the target, so that fixture checks only the refusal.
* Hydrogen counts: a Chemifree query atom with a stated count (a bracket atom
  such as ``[CH3]``) compares non-vertex hydrogens, taken from the valence
  model for an unstated target atom. RDKit's ``H`` primitive counts total
  hydrogens, which is the same quantity when no hydrogen is drawn as an atom.
  A target whose hydrogens Chemifree cannot determine (aromatic bonds in
  recorded mode) is a Chemifree-only refusal. Radicals are not compared: RDKit
  SMARTS has no radical primitive in this version.
* N-H heterocycles written in Kekule form with an unstated N-H (pyrazole,
  pyrrole) are refused in aromatic-target mode because the aromaticity model
  would otherwise read the nitrogen as pyridine-type; RDKit finds matches, so
  only the refusal is checked.
* Limits, refusals and completeness flags are Chemifree-only concepts and are
  checked against the declared expectation, not against RDKit.
* A seeded random section compares exact mapping sets for connected query
  subgraphs of random targets (recorded mode on C/N/O graphs, aromatic-target
  mode on substituted benzene written in both spellings), using RDKit's
  molecule-as-query matching.
"""

from __future__ import annotations

from collections.abc import Sequence
import dataclasses
import json
import logging
import pathlib
import random
import subprocess
import sys

import rdkit
import rdkit.Chem

_LOGGER = logging.getLogger(__name__)

_BOND_NAMES = {
    rdkit.Chem.BondType.SINGLE: "single",
    rdkit.Chem.BondType.DOUBLE: "double",
    rdkit.Chem.BondType.TRIPLE: "triple",
    rdkit.Chem.BondType.AROMATIC: "aromatic",
}
_MAX_MATCHES = 1000
_MAX_STATES = 1_000_000
_DEFAULT_STATES = 100_000


@dataclasses.dataclass(frozen=True)
class Fixture:
    """One comparison case.

    Attributes:
      name: Unique readable name.
      query: SMILES of the query graph (read without sanitization).
      smarts: Equivalent explicit SMARTS, atom for atom with `query`. Empty
        when the case is Chemifree-only.
      target: SMILES of the target graph.
      mode: Chemifree match model: "recorded" or "aromatic_target".
      predicates: Chemifree atom predicates by 0-based query atom position.
      refusal: Expected refusal code, or None when mappings are expected.
      limit: Maximum mappings requested, when below the full set.
      states: Search work bound requested (the service default).
    """

    name: str
    query: str
    smarts: str
    target: str
    mode: str = "recorded"
    predicates: tuple[dict[str, object], ...] = ()
    refusal: str | None = None
    limit: int = _MAX_MATCHES
    states: int = _DEFAULT_STATES


_BENZENE_KEKULE = "C1=CC=CC=C1"
_BENZENE_AROMATIC = "c1ccccc1"
_CC = "[#6+0]-[#6+0]"
_AROMATIC_PAIR = "[#6+0]:[#6+0]"
_FIXTURES = (
    Fixture("ethane_in_propane", "CC", _CC, "CCC"),
    Fixture("propane_in_ethane_no_match", "CCC", "[#6+0]-[#6+0]-[#6+0]", "CC"),
    Fixture(
        "benzene_in_benzene_aromatic_spelling",
        _BENZENE_AROMATIC,
        "[#6+0]1:[#6+0]:[#6+0]:[#6+0]:[#6+0]:[#6+0]:1",
        _BENZENE_AROMATIC,
    ),
    Fixture(
        "kekule_benzene_in_kekule_benzene_recorded",
        _BENZENE_KEKULE,
        "[#6+0]1=[#6+0]-[#6+0]=[#6+0]-[#6+0]=[#6+0]-1",
        _BENZENE_KEKULE,
    ),
    Fixture(
        "aromatic_bond_in_kekule_benzene_via_model",
        "cc",
        _AROMATIC_PAIR,
        _BENZENE_KEKULE,
        mode="aromatic_target",
    ),
    Fixture(
        "aromatic_bond_in_aromatic_benzene_via_model",
        "cc",
        _AROMATIC_PAIR,
        _BENZENE_AROMATIC,
        mode="aromatic_target",
    ),
    Fixture(
        "aromatic_bond_in_kekule_benzene_recorded_no_match",
        "cc",
        _AROMATIC_PAIR,
        _BENZENE_KEKULE,
    ),
    Fixture(
        "aromatic_query_pyridine_via_model",
        "cc",
        _AROMATIC_PAIR,
        "C1=CC=NC=C1",
        mode="aromatic_target",
    ),
    Fixture(
        "aromatic_nitrogen_atom_in_pyridine_via_model",
        "n",
        "[#7+0]",
        "c1ccncc1",
        mode="aromatic_target",
    ),
    Fixture(
        "naphthalene_benzene_ring_via_model",
        _BENZENE_AROMATIC,
        "[#6+0]1:[#6+0]:[#6+0]:[#6+0]:[#6+0]:[#6+0]:1",
        "C1=CC=C2C=CC=CC2=C1",
        mode="aromatic_target",
    ),
    Fixture(
        "double_bond_symmetric_ethene_in_butadiene",
        "C=C",
        "[#6+0]=[#6+0]",
        "C=CC=C",
    ),
    Fixture(
        "triple_bond_in_propyne",
        "C#C",
        "[#6+0]#[#6+0]",
        "CC#C",
    ),
    Fixture(
        "element_must_match_ethanol_oxygen",
        "CO",
        "[#6+0]-[#8+0]",
        "CCO",
    ),
    Fixture(
        "charged_target_atom_not_matched_by_neutral_query",
        "CN",
        "[#6+0]-[#7+0]",
        "C[N+](C)(C)C",
    ),
    Fixture(
        "charged_query_atom_matches_charged_target",
        "C[N+]",
        "[#6+0]-[#7+1]",
        "C[N+](C)(C)C",
    ),
    Fixture(
        "element_list_nitrogen_or_oxygen",
        "C*",
        "[#6+0]-[#7,#8]",
        "CCOCN",
    ),
    Fixture(
        "ring_condition_atom_in_ring_methylcyclohexane",
        "CC",
        "[#6+0;R]-[#6+0;R]",
        "CC1CCCCC1",
        predicates=(
            {"atom": 0, "ring": True},
            {"atom": 1, "ring": True},
        ),
    ),
    Fixture(
        "ring_condition_atom_outside_ring_methylcyclohexane",
        "CC",
        "[#6+0;!R]-[#6+0;R]",
        "CC1CCCCC1",
        predicates=(
            {"atom": 0, "ring": False},
            {"atom": 1, "ring": True},
        ),
    ),
    Fixture(
        "neighbor_count_branch_atom_isobutane",
        "C",
        "[#6+0;D{3-}]",
        "CC(C)C",
        predicates=({"atom": 0, "min_neighbors": 3},),
    ),
    Fixture(
        "neighbor_count_bounded_range_isopentane",
        "C",
        "[#6+0;D{2-2}]",
        "CC(C)CC",
        predicates=({"atom": 0, "min_neighbors": 2, "max_neighbors": 2},),
    ),
    Fixture(
        "charge_condition_on_any_atom",
        "*",
        "[*;+1]",
        "C[N+](C)(C)C.[Cl-]",
        predicates=({"atom": 0, "charge": 1},),
    ),
    Fixture(
        "disconnected_query_two_carbons_in_propane",
        "C.C",
        "[#6+0].[#6+0]",
        "CCC",
    ),
    Fixture(
        "fused_ring_spiro_target_cyclopropane_in_spiropentane",
        "C1CC1",
        "[#6+0]1-[#6+0]-[#6+0]-1",
        "C1CC12CC2",
    ),
    Fixture("no_match_element_absent", "N", "[#7+0]", "CCC"),
    Fixture(
        "mapping_limit_reports_incomplete",
        _BENZENE_AROMATIC,
        "[#6+0]1:[#6+0]:[#6+0]:[#6+0]:[#6+0]:[#6+0]:1",
        _BENZENE_AROMATIC,
        limit=3,
    ),
    Fixture(
        "hydrogens_methyl_in_ethane",
        "[CH3]",
        "[#6+0;H3]",
        "CC",
    ),
    Fixture(
        "hydrogens_methylene_in_propane",
        "[CH2]",
        "[#6+0;H2]",
        "CCC",
    ),
    Fixture(
        "hydrogens_methyl_absent_in_kekule_benzene",
        "[CH3]",
        "[#6+0;H3]",
        _BENZENE_KEKULE,
    ),
    Fixture(
        "hydrogens_ch_in_kekule_benzene_aromatic_model",
        "[CH]",
        "[#6+0;H1]",
        _BENZENE_KEKULE,
        mode="aromatic_target",
    ),
    Fixture(
        "isotope_label_matches_only_labeled_atom",
        "[13C]",
        "[13#6+0]",
        "[13CH3]C",
    ),
    Fixture(
        "refusal_stated_hydrogens_on_recorded_aromatic_target",
        "[CH]",
        "",
        _BENZENE_AROMATIC,
        refusal="target_hydrogens_undetermined",
    ),
    Fixture(
        "refusal_charge_condition_contradicts_element_atom",
        "C",
        "",
        "CC",
        predicates=({"atom": 0, "charge": 1},),
        refusal="invalid_predicate",
    ),
    Fixture(
        "refusal_kekule_pyrazole_unstated_nh",
        "cc",
        "",
        "C1=CC=NN1",
        mode="aromatic_target",
        refusal="aromatic_target_unsupported",
    ),
    Fixture(
        "refusal_kekule_pyrrole_unstated_nh",
        "cc",
        "",
        "C1=CC=CN1",
        mode="aromatic_target",
        refusal="aromatic_target_unsupported",
    ),
    Fixture(
        "two_atom_query_in_400_atom_chain_default_budget",
        "CC",
        _CC,
        "C" * 400,
    ),
    Fixture(
        "path_of_64_in_240_atom_chain_maximum_budget",
        "C" * 64,
        "-".join(["[#6+0]"] * 64),
        "C" * 240,
        states=_MAX_STATES,
    ),
    Fixture(
        "refusal_aromatic_target_cyclobutadiene",
        "cc",
        "",
        "c1ccc1",
        mode="aromatic_target",
        refusal="aromatic_target_unsupported",
    ),
    Fixture("refusal_empty_query", "", "", "CC", refusal="empty_query"),
    Fixture(
        "refusal_invalid_predicate_atom",
        "C",
        "",
        "CC",
        predicates=({"atom": 5, "ring": True},),
        refusal="invalid_predicate",
    ),
    Fixture(
        "refusal_zero_mapping_bound",
        "C",
        "",
        "CC",
        refusal="invalid_bounds",
        limit=0,
    ),
)


def _read_graph(smiles: str) -> dict[str, object]:
    """Reads a SMILES as written (no sanitization) into a request graph."""
    if not smiles:
        return {"atoms": [], "bonds": []}
    mol = rdkit.Chem.MolFromSmiles(smiles, sanitize=False)
    if mol is None:
        raise ValueError(f"RDKit cannot read {smiles=}")
    atoms: list[dict[str, object]] = []
    for atom in mol.GetAtoms():
        if atom.GetSymbol() == "*":
            atoms.append({"query": "any"})
        else:
            spec: dict[str, object] = {
                "el": atom.GetSymbol(),
                "charge": atom.GetFormalCharge(),
            }
            if atom.GetNumExplicitHs() > 0:
                spec["hydrogens"] = atom.GetNumExplicitHs()
            if atom.GetIsotope() > 0:
                spec["isotope"] = atom.GetIsotope()
            atoms.append(spec)
    bonds = [
        [
            bond.GetBeginAtomIdx(),
            bond.GetEndAtomIdx(),
            _BOND_NAMES[bond.GetBondType()],
        ]
        for bond in mol.GetBonds()
    ]
    return {"atoms": atoms, "bonds": bonds}


def _query_graph(fixture: Fixture) -> dict[str, object]:
    graph = _read_graph(fixture.query)
    if "[#7,#8]" in fixture.smarts:
        # SMILES cannot spell an element list; the list is Chemifree's
        # equivalent of the SMARTS alternative on the second atom.
        atoms = graph["atoms"]
        if not isinstance(atoms, list):
            raise TypeError("Request atoms must be a list")
        atoms[1] = {"query": "list", "elements": ["N", "O"]}
    return graph


def _run_chemifree(binary: pathlib.Path, fixture: Fixture) -> dict[str, object]:
    request = {
        "query": _query_graph(fixture),
        "target": _read_graph(fixture.target),
        "mode": fixture.mode,
        "predicates": list(fixture.predicates),
        "max_matches": fixture.limit,
        "max_states": fixture.states,
    }
    # The caller supplies the local example binary; no shell is involved.
    output = subprocess.run(  # noqa: S603
        [str(binary)],
        input=json.dumps(request),
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    response: object = json.loads(output.stdout)
    if not isinstance(response, dict):
        raise TypeError(f"Not a JSON object: {output.stdout!r}")
    return response


def _rdkit_matches(
    fixture: Fixture, *, uniquify: bool
) -> list[tuple[int, ...]]:
    """Returns RDKit's matches for the fixture, in RDKit's own atom order."""
    if fixture.mode == "aromatic_target":
        target = rdkit.Chem.MolFromSmiles(fixture.target)
    else:
        target = rdkit.Chem.MolFromSmiles(fixture.target, sanitize=False)
        target.UpdatePropertyCache(strict=False)
        rdkit.Chem.FastFindRings(target)
    pattern = rdkit.Chem.MolFromSmarts(fixture.smarts)
    if target is None or pattern is None:
        raise ValueError(f"RDKit cannot read {fixture.name}")
    return [
        tuple(match)
        for match in target.GetSubstructMatches(
            pattern, uniquify=uniquify, maxMatches=_MAX_MATCHES
        )
    ]


def _fail(fixture: Fixture, message: str) -> None:
    raise AssertionError(f"{fixture.name}: {message}")


def _check_refusal(fixture: Fixture, response: dict[str, object]) -> None:
    expected = {"outcome": "refused", "code": fixture.refusal}
    if response != expected:
        _fail(fixture, f"expected {expected}, got {response}")


def _mappings(response: dict[str, object]) -> list[list[int]]:
    mappings = response["mappings"]
    if not isinstance(mappings, list):
        raise TypeError(f"Mappings must be a list: {response}")
    return mappings


def _check_mappings(fixture: Fixture, response: dict[str, object]) -> None:
    if response.get("outcome") != "ok":
        _fail(fixture, f"unexpected refusal: {response}")
    mappings = _mappings(response)
    chemifree = {tuple(mapping) for mapping in mappings}
    if len(chemifree) != len(mappings):
        _fail(fixture, "Chemifree returned a duplicate mapping")
    full = set(_rdkit_matches(fixture, uniquify=False))
    if fixture.limit < _MAX_MATCHES and len(full) > fixture.limit:
        if response["complete"] is not False:
            _fail(fixture, "a truncated search was reported complete")
        if len(chemifree) != fixture.limit or not chemifree <= full:
            _fail(fixture, f"limited result {chemifree} is not a subset")
        return
    if response["complete"] is not True:
        _fail(fixture, "an exhaustive search was reported incomplete")
    if chemifree != full:
        _fail(
            fixture,
            f"mapping sets differ: only Chemifree {sorted(chemifree - full)},"
            f" only RDKit {sorted(full - chemifree)}",
        )
    unique_atom_sets = {frozenset(mapping) for mapping in chemifree}
    reference_unique = len(_rdkit_matches(fixture, uniquify=True))
    if len(unique_atom_sets) != reference_unique:
        _fail(
            fixture,
            f"unique atom sets {len(unique_atom_sets)} != RDKit"
            f" uniquified {reference_unique}",
        )


# Mapping counts that are known from chemistry alone, so a shared bug in the
# comparison plumbing cannot make both programs agree on a wrong answer.
_EXPECTED_COUNTS = {
    "ethane_in_propane": 4,
    "propane_in_ethane_no_match": 0,
    "benzene_in_benzene_aromatic_spelling": 12,
    "kekule_benzene_in_kekule_benzene_recorded": 6,
    "aromatic_bond_in_kekule_benzene_via_model": 12,
    "aromatic_bond_in_aromatic_benzene_via_model": 12,
    "aromatic_bond_in_kekule_benzene_recorded_no_match": 0,
    "aromatic_query_pyridine_via_model": 8,
    "naphthalene_benzene_ring_via_model": 24,
    "aromatic_nitrogen_atom_in_pyridine_via_model": 1,
    "double_bond_symmetric_ethene_in_butadiene": 4,
    "triple_bond_in_propyne": 2,
    "element_must_match_ethanol_oxygen": 1,
    "charged_target_atom_not_matched_by_neutral_query": 0,
    "charged_query_atom_matches_charged_target": 4,
    "element_list_nitrogen_or_oxygen": 3,
    "ring_condition_atom_in_ring_methylcyclohexane": 12,
    "ring_condition_atom_outside_ring_methylcyclohexane": 1,
    "neighbor_count_branch_atom_isobutane": 1,
    "neighbor_count_bounded_range_isopentane": 1,
    "charge_condition_on_any_atom": 1,
    "disconnected_query_two_carbons_in_propane": 6,
    "fused_ring_spiro_target_cyclopropane_in_spiropentane": 12,
    "no_match_element_absent": 0,
    "mapping_limit_reports_incomplete": 3,
    "hydrogens_methyl_in_ethane": 2,
    "hydrogens_methylene_in_propane": 1,
    "hydrogens_methyl_absent_in_kekule_benzene": 0,
    "hydrogens_ch_in_kekule_benzene_aromatic_model": 6,
    "isotope_label_matches_only_labeled_atom": 1,
    "two_atom_query_in_400_atom_chain_default_budget": 798,
    "path_of_64_in_240_atom_chain_maximum_budget": 354,
}


def _check(binary: pathlib.Path, fixture: Fixture) -> None:
    response = _run_chemifree(binary, fixture)
    if fixture.refusal is not None:
        _check_refusal(fixture, response)
        return
    _check_mappings(fixture, response)
    mappings = _mappings(response)
    expected = _EXPECTED_COUNTS[fixture.name]
    if len(mappings) != expected:
        _fail(fixture, f"{len(mappings)} mappings, chemistry says {expected}")


_VALENCE = {"C": 4, "N": 3, "O": 2}
_ORDER_NAMES = {1: "single", 2: "double", 3: "triple"}
_ORDER_TYPES = {
    1: rdkit.Chem.BondType.SINGLE,
    2: rdkit.Chem.BondType.DOUBLE,
    3: rdkit.Chem.BondType.TRIPLE,
}
_RANDOM_SEED = 20261101
_RANDOM_RECORDED_CASES = 40
_RANDOM_AROMATIC_CASES = 16

Edge = tuple[int, int, int]


def _random_molecule(rng: random.Random) -> tuple[list[str], list[Edge]]:
    """Builds a connected C/N/O graph whose atoms respect simple valences."""
    count = rng.randint(5, 9)
    elements = [rng.choice("CCCNO") for _ in range(count)]
    free = [_VALENCE[symbol] for symbol in elements]
    edges: list[Edge] = []
    for atom in range(1, count):
        candidates = [other for other in range(atom) if free[other] > 0]
        while free[atom] == 0 or not candidates:
            # Re-draw an element that cannot bond so the graph stays connected.
            elements[atom] = "C"
            free[atom] = 4 - sum(1 for e in edges if atom in e[:2])
            candidates = [other for other in range(atom) if free[other] > 0]
            if not candidates:
                other = rng.randrange(atom)
                elements[other] = "C"
                free[other] += 1
                candidates = [other]
        other = rng.choice(candidates)
        order = rng.randint(1, min(free[atom], free[other], 3))
        edges.append((other, atom, order))
        free[other] -= order
        free[atom] -= order
    for _ in range(rng.randint(0, 2)):
        pair = rng.sample(range(count), 2)
        linked = any(set(pair) == set(edge[:2]) for edge in edges)
        if not linked and all(free[atom] > 0 for atom in pair):
            edges.append((pair[0], pair[1], 1))
            free[pair[0]] -= 1
            free[pair[1]] -= 1
    return elements, edges


def _connected_subgraph(
    rng: random.Random, edges: Sequence[Edge], atom_count: int
) -> tuple[list[int], list[Edge]]:
    """Picks a random connected atom subset and some of its edges."""
    size = rng.randint(2, min(5, atom_count))
    chosen = [rng.randrange(atom_count)]
    tree: list[Edge] = []
    while len(chosen) < size:
        options = [
            edge for edge in edges if (edge[0] in chosen) != (edge[1] in chosen)
        ]
        if not options:
            break
        edge = rng.choice(options)
        chosen.append(edge[1] if edge[0] in chosen else edge[0])
        tree.append(edge)
    extra = [
        edge
        for edge in edges
        if edge[0] in chosen and edge[1] in chosen and edge not in tree
    ]
    return chosen, tree + [edge for edge in extra if rng.random() < 0.5]


def _graph_dict(
    elements: Sequence[str], edges: Sequence[tuple[int, int, str]]
) -> dict[str, object]:
    return {
        "atoms": [{"el": symbol, "charge": 0} for symbol in elements],
        "bonds": [[a, b, name] for a, b, name in edges],
    }


def _rdkit_molecule(
    elements: Sequence[str],
    edges: Sequence[tuple[int, int, rdkit.Chem.BondType]],
) -> rdkit.Chem.RWMol:
    mol = rdkit.Chem.RWMol()
    for symbol in elements:
        mol.AddAtom(rdkit.Chem.Atom(symbol))
    for a, b, bond_type in edges:
        mol.AddBond(a, b, bond_type)
    return mol


@dataclasses.dataclass(frozen=True)
class RandomCase:
    """One generated comparison: the same graphs for both programs.

    Attributes:
      name: Readable identifier including the seed position.
      mode: Chemifree match model.
      query: Query elements and bonds as (begin, end, order name).
      target: Target elements and bonds as (begin, end, order name).
      rdkit_query: The query as an RDKit molecule used as a query.
      rdkit_target: The target as an RDKit molecule.
    """

    name: str
    mode: str
    query: tuple[Sequence[str], Sequence[tuple[int, int, str]]]
    target: tuple[Sequence[str], Sequence[tuple[int, int, str]]]
    rdkit_query: rdkit.Chem.Mol
    rdkit_target: rdkit.Chem.Mol


def _compare_random(binary: pathlib.Path, case: RandomCase) -> int:
    """Checks one random case; returns the number of mappings compared."""
    request = {
        "query": _graph_dict(*case.query),
        "target": _graph_dict(*case.target),
        "mode": case.mode,
        "predicates": [],
        "max_matches": _MAX_MATCHES,
        "max_states": _MAX_STATES,
    }
    output = subprocess.run(  # noqa: S603
        [str(binary)],
        input=json.dumps(request),
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    response: object = json.loads(output.stdout)
    if not isinstance(response, dict) or response.get("outcome") != "ok":
        raise AssertionError(f"{case.name}: unexpected response {response}")
    expected = {
        tuple(match)
        for match in case.rdkit_target.GetSubstructMatches(
            case.rdkit_query, uniquify=False, maxMatches=_MAX_MATCHES
        )
    }
    if len(expected) >= _MAX_MATCHES:
        raise AssertionError(f"{case.name}: RDKit reached its match cap")
    found = {tuple(mapping) for mapping in response["mappings"]}
    if response["complete"] is not True or found != expected:
        raise AssertionError(
            f"{case.name}: Chemifree {sorted(found)} (complete="
            f"{response['complete']}) != RDKit {sorted(expected)}"
        )
    return len(found)


def _check_random_recorded(binary: pathlib.Path, rng: random.Random) -> int:
    compared = 0
    for case in range(_RANDOM_RECORDED_CASES):
        elements, edges = _random_molecule(rng)
        chosen, kept = _connected_subgraph(rng, edges, len(elements))
        order = list(range(len(chosen)))
        rng.shuffle(order)  # renumber query atoms to exercise numbering
        position = {atom: order[i] for i, atom in enumerate(chosen)}
        query_elements = [""] * len(chosen)
        for atom, new in position.items():
            query_elements[new] = elements[atom]
        query_edges = [(position[a], position[b], o) for a, b, o in kept]
        target = (elements, [(a, b, _ORDER_NAMES[o]) for a, b, o in edges])
        query = (
            query_elements,
            [(a, b, _ORDER_NAMES[o]) for a, b, o in query_edges],
        )
        rdkit_target = _rdkit_molecule(
            elements, [(a, b, _ORDER_TYPES[o]) for a, b, o in edges]
        )
        rdkit_target.UpdatePropertyCache(strict=False)
        rdkit_query = _rdkit_molecule(
            query_elements, [(a, b, _ORDER_TYPES[o]) for a, b, o in query_edges]
        )
        compared += _compare_random(
            binary,
            RandomCase(
                f"random_recorded_{case}",
                "recorded",
                query,
                target,
                rdkit_query,
                rdkit_target,
            ),
        )
    return compared


def _check_random_aromatic(binary: pathlib.Path, rng: random.Random) -> int:
    compared = 0
    for case in range(_RANDOM_AROMATIC_CASES):
        elements = ["C"] * 6
        substituents: list[tuple[int, int]] = []
        for ring_atom in rng.sample(range(6), rng.randint(0, 3)):
            elements.append(rng.choice("CNO"))
            substituents.append((ring_atom, len(elements) - 1))
            if elements[-1] != "O" and rng.random() < 0.4:
                elements.append("C")
                substituents.append((len(elements) - 2, len(elements) - 1))
        ring = [(i, (i + 1) % 6) for i in range(6)]
        shift = rng.randint(0, 1)
        aromatic_edges = [(a, b, "aromatic") for a, b in ring] + [
            (a, b, "single") for a, b in substituents
        ]
        kekule_edges = [
            (a, b, "double" if (i + shift) % 2 == 0 else "single")
            for i, (a, b) in enumerate(ring)
        ] + [(a, b, "single") for a, b in substituents]
        chosen, kept = _connected_subgraph(
            rng,
            [(a, b, 1) for a, b, _ in aromatic_edges],
            len(elements),
        )
        names = {(a, b): name for a, b, name in aromatic_edges}
        query_edges = [(a, b, names[(a, b)]) for a, b, _ in kept]
        index = {atom: i for i, atom in enumerate(chosen)}
        query = (
            [elements[atom] for atom in chosen],
            [(index[a], index[b], name) for a, b, name in query_edges],
        )
        types = {
            "single": rdkit.Chem.BondType.SINGLE,
            "double": rdkit.Chem.BondType.DOUBLE,
            "aromatic": rdkit.Chem.BondType.AROMATIC,
        }
        rdkit_query = _rdkit_molecule(
            query[0], [(a, b, types[name]) for a, b, name in query[1]]
        )
        # RDKit perceives aromaticity from the Kekule form on sanitization.
        rdkit_target = _rdkit_molecule(
            elements, [(a, b, types[name]) for a, b, name in kekule_edges]
        )
        rdkit.Chem.SanitizeMol(rdkit_target)
        for spelling, edges in (
            ("kekule", kekule_edges),
            ("aromatic", aromatic_edges),
        ):
            compared += _compare_random(
                binary,
                RandomCase(
                    f"random_aromatic_{case}_{spelling}",
                    "aromatic_target",
                    query,
                    (elements, edges),
                    rdkit_query,
                    rdkit_target,
                ),
            )
    return compared


def main(argv: Sequence[str]) -> None:
    """Runs every fixture; any mismatch or unexpected outcome raises."""
    logging.basicConfig(
        level=logging.INFO, format="%(message)s", stream=sys.stdout
    )
    if len(argv) != 2:
        raise SystemExit(
            "usage: verify-query-rdkit.py /absolute/path/to/query_reference"
        )
    binary = pathlib.Path(argv[1]).resolve(strict=True)
    names = [fixture.name for fixture in _FIXTURES]
    if len(set(names)) != len(names):
        raise ValueError("Fixture names must be unique")
    _LOGGER.info("RDKit version: %s", rdkit.__version__)
    for fixture in _FIXTURES:
        _check(binary, fixture)
    # Seeded for reproducibility, not for security.
    rng = random.Random(_RANDOM_SEED)  # noqa: S311
    recorded = _check_random_recorded(binary, rng)
    aromatic = _check_random_aromatic(binary, rng)
    _LOGGER.info(
        "Seeded random sections compared %d recorded and %d aromatic-target"
        " mappings",
        recorded,
        aromatic,
    )
    _LOGGER.info(
        "RDKit substructure comparison passed for %d fixtures", len(_FIXTURES)
    )


if __name__ == "__main__":
    main(sys.argv)
