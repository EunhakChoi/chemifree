# P35q retained fragment display script-run candidate

This follow-up candidate adds explicit baseline/subscript/superscript runs to an
existing retained fragment display. `set_fragment_display_runs` requires instance,
anchor and the complete `runs: [{ text, script }]` list. Every run must have
nonempty text and an explicit supported script. Concatenated text must contain a
non-whitespace character, remain a single Unicode line without controls or line
separators, and fit 128 UTF-8 bytes total. Empty lists/runs, all-blank text and
over-limit input use `display_label_invalid`; a missing display uses
`fragment_display_invalid`. Malformed fields and unknown/null scripts use
`invalid_request`. There is no caption-derived 32-run limit: nonempty runs and
the byte budget bound their count to at most128. Run boundaries and original text
are preserved without merging or normalization.

`contract_fragment` and `set_fragment_display_text` set exactly one baseline run.
The same concatenated text can therefore change scripts and boundaries when sent
through the plain-text command. Undo restores the former runs, and an identical
complete run list is a no-op. Snapshot entries expose both runs and their exact
concatenated text. An older snapshot without runs is read as one baseline run;
invalid explicit runs or disagreement with text refuses before rendering. Native
file admission has its own strict version rules below.

The UI offers Unicode run inputs, script selection, add/remove and Apply, freezes
submitted run values for the request queue and gates run submission during IME
composition. Plain-text Apply states that it resets scripts to baseline. Every
run inherits document label font/size/weight/style/underline/outline/shadow.
Baseline uses the normal label size; subscript and superscript use existing
chemical-label small-size and vertical shift ratios. Actual SVG measurements
feed shared bond trimming, fit, hit and selection bounds, including outlines,
underlines and shadows. The complete label stays centered at its anchor. The UI
warns that run boundaries may split shaping and asks users to keep combining marks
and emoji sequences within a run. Cross-run shaping is not guaranteed.

Text and scripts do not define chemistry. Actual retained graph membership, IDs,
states, coordinates and stereo supply every scientific inspection and export.
P35p's one-placement/tree/single-boundary/default-member eligibility, overlap and
local-stereo refusals remain. Whole-member rigid motion retains the display;
partial member geometry or affected chemistry reveals it in the same transaction.
Moving only the outside neighbor's coordinates retains it. Ordinary atomicity,
revision checks, request replay, selection and Undo/Redo remain shared.

Native document/draft v25 requires runs without top-level text in each current
fragment display and every non-null before/after history state. Each run has
required text/script. v24 requires text without runs, including history-only
states; empty/null runs behind a v24 header and old text behind a v25 header
refuse. v24 current/undo/redo text migrates to exactly one baseline run without
changing membership or graph/history. Versions1–23 keep their empty-map migration
and forbid fragment fields. Reading writes nothing. Exact old document/draft bytes
are archived before first overwrite/recovery under the existing protocol, now
including v24. Future v26 refuses untouched. A single baseline run preserves v24
admission weights; extra runs reserve64 bytes each in current and both history
sides. Existing current48MiB/history8MiB admission and physical64MiB limits remain.

Chemical exports still write the complete graph with `abbreviation_display`
rendering loss; the display loss alone needs no consent, while unrelated existing
consent rules remain. Native clipboard, Cut preparation and graph-template export
still refuse member intersections until expansion. No dictionary, parser,
external run roundtrip or per-run font/color/weight/style override is introduced.
Independent captions and Et display-label editing retain their own contracts.

Authored/in-progress check scope includes domain run validation, reset/no-op,
rollback and Undo/Redo; application snapshot/replay and scientific invariance;
native v24/v25 current/undo/redo/draft strictness, legacy migration and exact-byte
archive; client run/text validation and baseline fallback; shared script-label
geometry and production browser UI/IME/history/save/reopen/actual SVG bounds.
These are source-inspected intended checks, not successful execution evidence.

Execution status: this documentation pass read current implementation source and
inspected document diffs only. Tests, builds, browser and quality gates have not
been run for this candidate by this work. Required Codex and independent fresh
subagent PR reviews, findings and dispositions are pending the parent workflow.
P35 remains `in_progress`; earlier merged PR/CI evidence does not establish this
candidate's verification or completion of general chemical text rendering.
