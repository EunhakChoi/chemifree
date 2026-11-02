# P21a aromaticity model revision 2: change record

This records every corpus input whose aromaticity classification, reason, conversion
outcome or chemical identity differs between `chemifree-aromaticity-1` (main at 8f2d40a,
with `chemifree-chemical-graph-1`) and `chemifree-aromaticity-2` (with
`chemifree-chemical-graph-2`). It is a prerequisite for P21 (SMILES). P21 itself is not
done and nothing here adds SMILES. The rules are defined once, in
[CONTRACT.md](../CONTRACT.md#ring-and-aromaticity-interpretation).

## Corpus and reproduction

The corpus has 196 MOL/V2000 records for 116 molecules, generated with RDKit 2022.03.2:
acyclic, saturated rings, benzene and monocycles, monocyclic and fused heteroaromatics,
benzenoids (up to coronene), fused partially saturated systems, bridged (norbornane,
norbornene, norbornadiene, alpha-pinene, adamantane, camphor), spiro, charged,
radicals and carbenes, hypervalent (N or S valence above the supported one), antiaromatic and large annulenes (cyclobutadiene, cyclooctatetraene, [10]- and
[18]annulene), azulene-like systems and exocyclic double bonds. Every molecule has a Kekule
spelling with no stated hydrogens; molecules RDKit perceives as aromatic also have an
aromatic spelling (the reader accepts only single, double and triple bonds, so the
molblock stays Kekule and a list names the bonds recorded as aromatic) in which aromatic
nitrogens with hydrogen carry a stated count, as in `[nH]`.

```bash
# corpus (committed as docs/verification/aromaticity-corpus.json)
python -I scripts/aromaticity_corpus.py docs/verification/aromaticity-corpus.json
# one run per checkout; the example uses only API that exists in both revisions
cargo run --locked -q -p chemifree-providers --example aromaticity_corpus -- \
  docs/verification/aromaticity-corpus.json > new.jsonl      # this branch
git worktree add --detach ../main-baseline 8f2d40a            # then copy the example in
cargo run --locked -q -p chemifree-providers --example aromaticity_corpus -- \
  <path>/docs/verification/aromaticity-corpus.json > old.jsonl # main
python -I scripts/aromaticity_corpus_diff.py old.jsonl new.jsonl
```

The tables at the end of this file are the verbatim output of that last command.

## Summary

196 records; 53 changed (27%); 143 unchanged. Counts per change kind (one record can
appear under several):

| change kind | records |
|---|---|
| classification or reason | 53 |
| chemical identity availability (accepted <-> refused, or refusal reason) | 45 |
| chemical identity detail (same availability, recorded identity differs) | 8 |
| conversion outcome (aromatic or Kekule conversion refused or accepted) | 45 |

By class (records changed / records): fused partially saturated 28/30, monocyclic
heteroaromatic 6/38, spiro 6/6, donor-only and sigma rings 6/9, fused heteroaromatic
5/20, bridged 1/9, charged 1/15.
Acyclic, saturated ring, benzene and monocycle, benzenoid, antiaromatic and large,
azulene-like and exocyclic double bond records are all unchanged.

What changed, grouped:

1. **Benzene fused to a saturated or non-conjugated ring (28 fused-partially-saturated and
   4 spiro records, 32 total).** Kekule spellings were `non_aromatic/not_conjugated` and
   their chemical identity was accepted (a complete but wrong answer). They are now
   `unsupported/fused_partially_aromatic` and identity is refused with that reason.
   Aromatic spellings were `unsupported/mixed_ring_bonds` and are now
   `unsupported/fused_partially_aromatic`: both spellings of one molecule agree. Kekule spellings of the spiro
   records (spirobenzene-cyclopentane, spirobifluorene) were `valence_not_in_model`,
   because a quaternary sp3 ring carbon was required to need a double bond; they now
   get the same classification as their aromatic spellings.
2. **Kekule ring nitrogen with inferred hydrogen, monocycle (7 records: pyrrole, pyrazole,
   imidazole, 1,2,3- and 1,2,4-triazole and tetrazole, plus imidazolium in the charged
   class).** Were `non_aromatic/not_conjugated` (pyridine-type nitrogen). Now `aromatic`,
   so their chemical identity code changes (7 records) and equals the aromatic spelling.
3. **Kekule fused heteroaromatics with such a nitrogen (indole, benzimidazole, indazole,
   carbazole, purine; 5 records).** Were `non_aromatic/not_conjugated` with an accepted
   identity; now `unsupported/fused_system_outside_model` (the model recognizes fused
   systems only for neutral carbocycles) and identity is refused. The aromatic spellings
   were and remain `unsupported/fused_system_outside_model`.
4. **Saturated bridged and spiro skeletons (alpha-pinene, spiro[4.4]nonane,
   spiro[4.5]deca-6,9-diene; 3 records).** Were `unsupported/valence_not_in_model` (a quaternary
   ring carbon); now `non_aromatic/not_conjugated`, identity accepted, aromatic conversion
   accepted as a no-op.
5. **Conversion outcomes (45 records).** Aromatic conversion is refused for any document
   containing a `fused_partially_aromatic` system, including "benzene plus tetralin
   (separate)", whose benzene main converted alone. Kekule conversion of a Kekule graph
   (nothing to convert) is unchanged.
6. **Donor-only and sigma-only rings (6 records, added in review round 1).** A ring
   whose atoms all donate but which has no double or aromatic bond has no conjugated
   circuit. A first draft of revision 2 called Kekule cyclotriazane and the O/S ring
   aromatic (a neutral nitrogen with inferred hydrogen is always a donor); the final rules
   require a π bond on a circuit. Against main: Kekule cyclotriazane, the triazane-fused
   ring and 1,2-dithiole are unchanged `non_aromatic`; the Kekule O/S ring was `aromatic`
   on main (ten donor electrons around single bonds) and is now `non_aromatic`; two adjacent donors beside a C=C
   (1,2-dihydro-1,2-diazete) and RDKit-aromatic-flagged all-donor rings are now
   `unsupported/adjacent_donors` (they were `aromatic` or `non_aromatic` on main).
7. **Radicals, carbenes and hypervalent atoms (6 records, unchanged against main).**
   Main refused them; an intermediate draft of revision 2 accepted them by trusting the
   valence model, so they are in the corpus as a permanent check. Every radical or carbene
   ring atom is `unsupported/radical_state` and a ring N or S above its supported total
   valence is `unsupported/valence_not_in_model`, in Kekule and aromatic spellings alike.

## User-visible changes to call out

- A document with one unsupported ring system can no longer be partially converted to
  aromatic bonds (main converted the supported systems when the unsupported one was
  judged non-aromatic). Pinned by `one_unsupported_system_refuses_the_whole_document_conversion`
  and the conformance case `aromaticity-partially-aromatic-fused-refused`.
- Aromatic conversion adds a hydrogen statement to Kekule pyrrole-type nitrogens, so a
  conversion that changes five bonds of a pyrrole is six commands. A conversion that
  needs more than 500 commands is refused whole, with the reason `conversion_command_limit`
  (not the generic transaction message).
- Chemical identity changes for the records above. Those are changes to classifications,
  identities and conversions that succeeded on revision 1, which is why the model ids were
  bumped to `chemifree-aromaticity-2` and `chemifree-chemical-graph-2` so stored
  comparisons are not confused. The rule from then on, under one model id: every
  classification, identity and conversion that succeeds is identical, and a refusal
  (`unsupported`) may later become a result without a new id, so a stored refusal is only
  valid for the build that produced it (see the next section).

## Refusals turned into results after this record (issue 93)

Two refusals of the revision-2 record above were narrowed afterwards. Under one model id
every classification, identity and conversion that succeeds is identical, and a refusal
(`unsupported`) may later become a result without a new id, so a stored refusal is only
valid for the build that produced it. 1,2-dithiin is the example: it was refused
(`adjacent_donors`) in the first builds of revision 2 and is `non_aromatic` now. Neither
narrowing changes a classification, identity or conversion that succeeded before, so the model ids stay
`chemifree-aromaticity-2` and `chemifree-chemical-graph-2`, and the generated tables below
are unchanged (the corpus gives the same output before and after; the corpus has no input
of either kind). Both restore the answer `chemifree-aromaticity-1` gave.

- **Adjacent donors are decided after the electron count.** A single circuit with adjacent
  donors whose count (every donor as two electrons) is not 4n+2 is `non_aromatic`
  (`electron_count`) instead of `unsupported` (`adjacent_donors`): 1,2-dithiin (8 electrons)
  is `non_aromatic` again, so it no longer blocks whole-document conversion; the same holds
  for the O/S and N/N analogues. A Kekule double-bond pattern that cannot alternate is
  `non_aromatic` (`not_conjugated`) before adjacent donors are considered. A count of 4n+2
  (1,2-dihydro-1,2-diazete, cyclotriazane in aromatic spelling and the O/S ring above, 6 and
  10 electrons) and any fused block stay `unsupported` (`adjacent_donors`), as in the tables
  below, because the model does not decide that lone-pair arrangement.
- **A two-coordinate ring nitrogen with a stated hydrogen count of 0 and only single bonds**
  (valence 2) is outside the conjugated system again, like a saturated atom: a block with no
  remaining conjugated cycle is `non_aromatic` (`not_conjugated`) instead of `unsupported`
  (`valence_not_in_model`). Beside an aromatic sub-ring it is `unsupported`
  (`fused_partially_aromatic`), like a secondary amine, and with aromatic bonds on the
  other atoms it is `unsupported` (`mixed_ring_bonds`). Its
  chemical identity is still refused (`unusual_valence`).

Two reasons differ between refusals; no result does. The `mixed_ring_bonds` check still
runs after the atom roles are assessed, so a ring that mixes bond orders and holds an atom
the model cannot read reports that atom's reason (`chemifree-aromaticity-1` reported
`mixed_ring_bonds`); running it first would make the aromatic spelling of tetralin-like
blocks report `mixed_ring_bonds` instead of `fused_partially_aromatic` (item 1 above). Among
the checks of a fully conjugated block `mixed_ring_bonds` now precedes `adjacent_donors`
(which only changes the reason of an input that is refused either way). A conversion over
the 500-command bound reports `conversion_command_limit` instead of the generic
transaction error.

## Detailed results (generated)

Records: 196; changed: 53; unchanged: 143

| class | records | changed |
|---|---|---|
| acyclic | 4 | 0 |
| antiaromatic and large | 7 | 0 |
| azulene-like | 8 | 0 |
| benzene and monocycles | 12 | 0 |
| benzenoid | 16 | 0 |
| bridged | 9 | 1 |
| charged | 15 | 1 |
| donor-only and sigma rings | 9 | 6 |
| exocyclic double bond | 6 | 0 |
| fused heteroaromatic | 20 | 5 |
| fused partially saturated | 30 | 28 |
| hypervalent | 2 | 0 |
| monocyclic heteroaromatic | 38 | 6 |
| radical | 4 | 0 |
| saturated ring | 10 | 0 |
| spiro | 6 | 6 |

### chemical identity availability (45)

| class | molecule | spelling | change |
|---|---|---|---|
| donor-only and sigma rings | 1,2-dihydro-1,2-diazete | aromatic | ok -> refused/adjacent_donors |
| donor-only and sigma rings | 1,2-dihydro-1,2-diazete | kekule | ok -> refused/adjacent_donors |
| fused partially saturated | 1,2-dihydronaphthalene | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | 1,2-dihydronaphthalene | kekule | ok -> refused/fused_partially_aromatic |
| fused partially saturated | 1,3-benzodioxole | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | 1,3-benzodioxole | kekule | ok -> refused/fused_partially_aromatic |
| fused partially saturated | 2,3-dihydrobenzofuran | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | 2,3-dihydrobenzofuran | kekule | ok -> refused/fused_partially_aromatic |
| fused partially saturated | 9,10-dihydroanthracene | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | 9,10-dihydroanthracene | kekule | ok -> refused/fused_partially_aromatic |
| bridged | alpha-pinene | kekule | refused/valence_not_in_model -> ok |
| fused partially saturated | benzene plus tetralin (separate) | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | benzene plus tetralin (separate) | kekule | ok -> refused/fused_partially_aromatic |
| fused heteroaromatic | benzimidazole | kekule | ok -> refused/fused_system_outside_model |
| fused heteroaromatic | carbazole | kekule | ok -> refused/fused_system_outside_model |
| fused partially saturated | chroman | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | chroman | kekule | ok -> refused/fused_partially_aromatic |
| donor-only and sigma rings | cyclotriazane | aromatic | ok -> refused/adjacent_donors |
| fused partially saturated | estratriene skeleton | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | estratriene skeleton | kekule | ok -> refused/fused_partially_aromatic |
| donor-only and sigma rings | five-membered O-S ring (trioxadithiolane) | aromatic | ok -> refused/adjacent_donors |
| fused partially saturated | fluorene | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | fluorene | kekule | ok -> refused/fused_partially_aromatic |
| fused partially saturated | indane | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | indane | kekule | ok -> refused/fused_partially_aromatic |
| fused heteroaromatic | indazole | kekule | ok -> refused/fused_system_outside_model |
| fused partially saturated | indene | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | indene | kekule | ok -> refused/fused_partially_aromatic |
| fused heteroaromatic | indole | kekule | ok -> refused/fused_system_outside_model |
| fused partially saturated | indoline | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | indoline | kekule | ok -> refused/fused_partially_aromatic |
| fused partially saturated | isoindoline | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | isoindoline | kekule | ok -> refused/fused_partially_aromatic |
| fused heteroaromatic | purine | kekule | ok -> refused/fused_system_outside_model |
| spiro | spiro[4.4]nonane | kekule | refused/valence_not_in_model -> ok |
| spiro | spiro[4.5]deca-6,9-diene | kekule | refused/valence_not_in_model -> ok |
| spiro | spirobenzene-cyclopentane | aromatic | refused/valence_not_in_model -> refused/fused_partially_aromatic |
| spiro | spirobenzene-cyclopentane | kekule | refused/valence_not_in_model -> refused/fused_partially_aromatic |
| spiro | spirobifluorene | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| spiro | spirobifluorene | kekule | refused/valence_not_in_model -> refused/fused_partially_aromatic |
| fused partially saturated | tetrahydroquinoline | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | tetrahydroquinoline | kekule | ok -> refused/fused_partially_aromatic |
| fused partially saturated | tetralin | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |
| fused partially saturated | tetralin | kekule | ok -> refused/fused_partially_aromatic |
| donor-only and sigma rings | triazane fused to CH2 ring | aromatic | refused/mixed_ring_bonds -> refused/fused_partially_aromatic |

### chemical identity detail (8)

| class | molecule | spelling | change |
|---|---|---|---|
| monocyclic heteroaromatic | 1,2,3-triazole | kekule | {"code": "330b2f98ee784572"} -> {"code": "63cdb94655aecb99"} |
| monocyclic heteroaromatic | 1,2,4-triazole | kekule | {"code": "480aec53990f54da"} -> {"code": "a240360e62197c31"} |
| donor-only and sigma rings | five-membered O-S ring (trioxadithiolane) | kekule | {"code": "a7bd12923ab65af3"} -> {"code": "511768196e3710d4"} |
| monocyclic heteroaromatic | imidazole | kekule | {"code": "668dd2ab423fdd52"} -> {"code": "d5b12a19a8830089"} |
| charged | imidazolium | kekule | {"code": "a3c69e6967c1c0da"} -> {"code": "3abb6b324e04a941"} |
| monocyclic heteroaromatic | pyrazole | kekule | {"code": "792faf5d6c36950a"} -> {"code": "371017afa86010e9"} |
| monocyclic heteroaromatic | pyrrole | kekule | {"code": "0b2e843c80d4df62"} -> {"code": "726e1f0176e68c89"} |
| monocyclic heteroaromatic | tetrazole | kekule | {"code": "eb9320d91e1700f2"} -> {"code": "0609c972f3a20eb9"} |

### classification or reason (53)

| class | molecule | spelling | change |
|---|---|---|---|
| monocyclic heteroaromatic | 1,2,3-triazole | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| monocyclic heteroaromatic | 1,2,4-triazole | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| donor-only and sigma rings | 1,2-dihydro-1,2-diazete | aromatic | aromatic (4 atoms) -> unsupported/adjacent_donors (4 atoms) |
| donor-only and sigma rings | 1,2-dihydro-1,2-diazete | kekule | non_aromatic/not_conjugated (4 atoms) -> unsupported/adjacent_donors (4 atoms) |
| fused partially saturated | 1,2-dihydronaphthalene | aromatic | unsupported/mixed_ring_bonds (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| fused partially saturated | 1,2-dihydronaphthalene | kekule | non_aromatic/not_conjugated (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| fused partially saturated | 1,3-benzodioxole | aromatic | unsupported/mixed_ring_bonds (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | 1,3-benzodioxole | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | 2,3-dihydrobenzofuran | aromatic | unsupported/mixed_ring_bonds (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | 2,3-dihydrobenzofuran | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | 9,10-dihydroanthracene | aromatic | unsupported/mixed_ring_bonds (14 atoms) -> unsupported/fused_partially_aromatic (14 atoms) |
| fused partially saturated | 9,10-dihydroanthracene | kekule | non_aromatic/not_conjugated (14 atoms) -> unsupported/fused_partially_aromatic (14 atoms) |
| bridged | alpha-pinene | kekule | unsupported/valence_not_in_model (7 atoms) -> non_aromatic/not_conjugated (7 atoms) |
| fused partially saturated | benzene plus tetralin (separate) | aromatic | unsupported/mixed_ring_bonds (10 atoms); aromatic (6 atoms) -> unsupported/fused_partially_aromatic (10 atoms); aromatic (6 atoms) |
| fused partially saturated | benzene plus tetralin (separate) | kekule | non_aromatic/not_conjugated (10 atoms); aromatic (6 atoms) -> unsupported/fused_partially_aromatic (10 atoms); aromatic (6 atoms) |
| fused heteroaromatic | benzimidazole | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_system_outside_model (9 atoms) |
| fused heteroaromatic | carbazole | kekule | non_aromatic/not_conjugated (13 atoms) -> unsupported/fused_system_outside_model (13 atoms) |
| fused partially saturated | chroman | aromatic | unsupported/mixed_ring_bonds (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| fused partially saturated | chroman | kekule | non_aromatic/not_conjugated (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| donor-only and sigma rings | cyclotriazane | aromatic | aromatic (3 atoms) -> unsupported/adjacent_donors (3 atoms) |
| fused partially saturated | estratriene skeleton | aromatic | unsupported/mixed_ring_bonds (17 atoms) -> unsupported/fused_partially_aromatic (17 atoms) |
| fused partially saturated | estratriene skeleton | kekule | non_aromatic/not_conjugated (17 atoms) -> unsupported/fused_partially_aromatic (17 atoms) |
| donor-only and sigma rings | five-membered O-S ring (trioxadithiolane) | aromatic | aromatic (5 atoms) -> unsupported/adjacent_donors (5 atoms) |
| donor-only and sigma rings | five-membered O-S ring (trioxadithiolane) | kekule | aromatic (5 atoms) -> non_aromatic/not_conjugated (5 atoms) |
| fused partially saturated | fluorene | aromatic | unsupported/mixed_ring_bonds (13 atoms) -> unsupported/fused_partially_aromatic (13 atoms) |
| fused partially saturated | fluorene | kekule | non_aromatic/not_conjugated (13 atoms) -> unsupported/fused_partially_aromatic (13 atoms) |
| monocyclic heteroaromatic | imidazole | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| charged | imidazolium | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| fused partially saturated | indane | aromatic | unsupported/mixed_ring_bonds (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | indane | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused heteroaromatic | indazole | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_system_outside_model (9 atoms) |
| fused partially saturated | indene | aromatic | unsupported/mixed_ring_bonds (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | indene | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused heteroaromatic | indole | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_system_outside_model (9 atoms) |
| fused partially saturated | indoline | aromatic | unsupported/mixed_ring_bonds (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | indoline | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | isoindoline | aromatic | unsupported/mixed_ring_bonds (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused partially saturated | isoindoline | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_partially_aromatic (9 atoms) |
| fused heteroaromatic | purine | kekule | non_aromatic/not_conjugated (9 atoms) -> unsupported/fused_system_outside_model (9 atoms) |
| monocyclic heteroaromatic | pyrazole | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| monocyclic heteroaromatic | pyrrole | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| spiro | spiro[4.4]nonane | kekule | unsupported/valence_not_in_model (5 atoms); unsupported/valence_not_in_model (5 atoms) -> non_aromatic/not_conjugated (5 atoms); non_aromatic/not_conjugated (5 atoms) |
| spiro | spiro[4.5]deca-6,9-diene | kekule | unsupported/valence_not_in_model (5 atoms); unsupported/valence_not_in_model (6 atoms) -> non_aromatic/not_conjugated (5 atoms); non_aromatic/not_conjugated (6 atoms) |
| spiro | spirobenzene-cyclopentane | aromatic | unsupported/valence_not_in_model (5 atoms); unsupported/mixed_ring_bonds (9 atoms) -> non_aromatic/not_conjugated (5 atoms); unsupported/fused_partially_aromatic (9 atoms) |
| spiro | spirobenzene-cyclopentane | kekule | unsupported/valence_not_in_model (5 atoms); unsupported/valence_not_in_model (9 atoms) -> non_aromatic/not_conjugated (5 atoms); unsupported/fused_partially_aromatic (9 atoms) |
| spiro | spirobifluorene | aromatic | unsupported/mixed_ring_bonds (13 atoms); unsupported/mixed_ring_bonds (13 atoms) -> unsupported/fused_partially_aromatic (13 atoms); unsupported/fused_partially_aromatic (13 atoms) |
| spiro | spirobifluorene | kekule | unsupported/valence_not_in_model (13 atoms); unsupported/valence_not_in_model (13 atoms) -> unsupported/fused_partially_aromatic (13 atoms); unsupported/fused_partially_aromatic (13 atoms) |
| fused partially saturated | tetrahydroquinoline | aromatic | unsupported/mixed_ring_bonds (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| fused partially saturated | tetrahydroquinoline | kekule | non_aromatic/not_conjugated (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| fused partially saturated | tetralin | aromatic | unsupported/mixed_ring_bonds (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| fused partially saturated | tetralin | kekule | non_aromatic/not_conjugated (10 atoms) -> unsupported/fused_partially_aromatic (10 atoms) |
| monocyclic heteroaromatic | tetrazole | kekule | non_aromatic/not_conjugated (5 atoms) -> aromatic (5 atoms) |
| donor-only and sigma rings | triazane fused to CH2 ring | aromatic | unsupported/mixed_ring_bonds (5 atoms) -> unsupported/fused_partially_aromatic (5 atoms) |

### conversion outcome (45)

| class | molecule | spelling | change |
|---|---|---|---|
| donor-only and sigma rings | 1,2-dihydro-1,2-diazete | aromatic | aromatic: ok, kekule: ok -> aromatic: adjacent_donors, kekule: adjacent_donors |
| donor-only and sigma rings | 1,2-dihydro-1,2-diazete | kekule | aromatic: ok, kekule: ok -> aromatic: adjacent_donors, kekule: ok |
| fused partially saturated | 1,2-dihydronaphthalene | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | 1,2-dihydronaphthalene | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | 1,3-benzodioxole | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | 1,3-benzodioxole | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | 2,3-dihydrobenzofuran | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | 2,3-dihydrobenzofuran | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | 9,10-dihydroanthracene | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | 9,10-dihydroanthracene | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| bridged | alpha-pinene | kekule | aromatic: valence_not_in_model, kekule: ok -> aromatic: ok, kekule: ok |
| fused partially saturated | benzene plus tetralin (separate) | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | benzene plus tetralin (separate) | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused heteroaromatic | benzimidazole | kekule | aromatic: ok, kekule: ok -> aromatic: fused_system_outside_model, kekule: ok |
| fused heteroaromatic | carbazole | kekule | aromatic: ok, kekule: ok -> aromatic: fused_system_outside_model, kekule: ok |
| fused partially saturated | chroman | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | chroman | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| donor-only and sigma rings | cyclotriazane | aromatic | aromatic: ok, kekule: ok -> aromatic: adjacent_donors, kekule: adjacent_donors |
| fused partially saturated | estratriene skeleton | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | estratriene skeleton | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| donor-only and sigma rings | five-membered O-S ring (trioxadithiolane) | aromatic | aromatic: ok, kekule: ok -> aromatic: adjacent_donors, kekule: adjacent_donors |
| fused partially saturated | fluorene | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | fluorene | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | indane | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | indane | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused heteroaromatic | indazole | kekule | aromatic: ok, kekule: ok -> aromatic: fused_system_outside_model, kekule: ok |
| fused partially saturated | indene | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | indene | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused heteroaromatic | indole | kekule | aromatic: ok, kekule: ok -> aromatic: fused_system_outside_model, kekule: ok |
| fused partially saturated | indoline | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | indoline | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | isoindoline | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | isoindoline | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused heteroaromatic | purine | kekule | aromatic: ok, kekule: ok -> aromatic: fused_system_outside_model, kekule: ok |
| spiro | spiro[4.4]nonane | kekule | aromatic: valence_not_in_model, kekule: ok -> aromatic: ok, kekule: ok |
| spiro | spiro[4.5]deca-6,9-diene | kekule | aromatic: valence_not_in_model, kekule: ok -> aromatic: ok, kekule: ok |
| spiro | spirobenzene-cyclopentane | aromatic | aromatic: valence_not_in_model, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| spiro | spirobenzene-cyclopentane | kekule | aromatic: valence_not_in_model, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| spiro | spirobifluorene | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| spiro | spirobifluorene | kekule | aromatic: valence_not_in_model, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | tetrahydroquinoline | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | tetrahydroquinoline | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| fused partially saturated | tetralin | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
| fused partially saturated | tetralin | kekule | aromatic: ok, kekule: ok -> aromatic: fused_partially_aromatic, kekule: ok |
| donor-only and sigma rings | triazane fused to CH2 ring | aromatic | aromatic: mixed_ring_bonds, kekule: mixed_ring_bonds -> aromatic: fused_partially_aromatic, kekule: fused_partially_aromatic |
