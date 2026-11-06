# P33 bounded clipboard completion

P33 is recorded as `done` against its roadmap scope and explicit acceptance
criteria: clipboard support for the declared internal-object and supported
chemical-text profiles, safe ID/style/containment handling across documents,
source preservation after paste, Undo restoration, permission-error feedback and
conversion-loss reporting. This is a bounded product-scope judgment. Broader
OPS-01, OPS-02 and CODEC requirements remain partial; it is not completion of
every internal object, format or legacy clipboard behavior.

[PR #172](https://github.com/EunhakChoi/chemifree/pull/172) merged as
`10822930dc7c6114944156eb15f0a9c5a424aa6a`. Its implementation head
`7e7c921fae0ce2e4f3e6f5ff8222ca2859698dca` passed the isolated checks and
[PR CI check/browser](https://github.com/EunhakChoi/chemifree/actions/runs/42148445439)
listed below. **Post-merge main CI remains pending.** That remaining delivery
verification must be checked and any failure resolved; it is not counted as a
success in this record. This completion documentation PR also awaits its own
two reviews and required CI. The documentation change runs no tests, builds or
browsers and makes no new mutation or review-completion claim.

## Delivered profiles and limitations

| Profile | Supported behavior | Explicit boundary |
| --- | --- | --- |
| Native graph, P33a/b/c | Explicit-placement graph Copy/Cut/Paste; selected atoms/bond endpoints and induced bonds; crossing-bond report; recorded properties, marks and complete stored 0D stereo; fresh atom/bond IDs and selected pasted targets | Version 1 data-only JSON, 64 KiB, 500-command/domain limits. No scene-object selection or enhanced-stereo-group member copying; missing stereo neighbors refuse. Paste needs exactly one destination placement and exactly equal settings. |
| Native graphic forest, P33e/f/j | Outermost lines/arrows/rectangles/groups, paint and child order, internal object references including self-links/cycles, fresh object IDs and selected pasted roots; multiple destination molecule placements remain untouched | Separate version 2 graphics profile, 64 KiB and existing resource limits, exact settings. Instances, mixed graph targets, outgoing/graph references and groups with fewer than two children refuse. |
| Native Cut, P33c/j | Activated successful clipboard write precedes one deletion under the exact prepared source snapshot/revision/process; Undo/Redo restores IDs, contents, references and captured selection | Graph Cut needs one source placement, no crossing bonds and no incoming atom/bond references. Forest Cut additionally rejects outside-owned incoming references. External parents are retained, including empty/singleton groups. Manual copy never deletes. Clipboard writing and deletion are separate; a refused deletion leaves a copy, with no rollback or automatic paste-back. |
| MOL text, P33d | Whole-drawing first-placement export using existing V2000/V3000/Auto loss consent; explicit read/manual input, preview and consent before existing-document append | Existing MOL profile/coordinate conversion, 200 KiB source limit and one destination placement. No native paste offsets or automatic imported-target selection. SDF clipboard is not provided. |
| SMILES text, P33g | Whole-graph recorded/canonical output after existing consent; explicit read/manual input, preview and existing-document append | Existing bounded grammar, 16 KiB source limit, one placement and deterministic import grid. Existing live selection/history is preserved; no new chemistry or layout inference. |
| CDXML text, P33h/i | Explicit prepared whole-drawing first-placement export copying; text import with explicit page/molecule selection, projection preview and loss acceptance creates a new drawing | 120 KiB input, existing CDXML projection only. No append to the current drawing; CDX stays binary-file import/download without base64 clipboard text. |

Graph and graphics profiles are deliberately separate operations and payloads.
Exact matching native style is a checked refusal policy, not automatic restyling
or rescaling. Molecule-instance cloning, mixed chemistry/graphics cloning,
enhanced-stereo-group graph copying, global Ctrl/Cmd+C/V/X interception, custom
MIME, format guessing, external-format Cut, Windows OLE and Office integration
are outside this bounded delivery. Windows host work remains separate W01/W02/W04
scope, not a prerequisite silently added to P33.

Browser reads/writes require explicit user actions. Denied or unavailable access
leaves manual text alternatives. Import/export losses retain the existing
preview/digest-consent rules. Uncertain mutations retain the exact request body,
ID and process, block later changes, and check only that attempt; restart or
terminal conflict requires explicit reconciliation rather than a new-process
resend. Clipboard payloads are not polled or persisted in browser storage.

## Acceptance evidence

| Roadmap criterion | Evidence and its limits |
| --- | --- |
| Paste across documents preserves the source | P33k runs production panels, hook and client through a real API process/Application/Domain with seeded source and destination documents. It checks full source content, revision and history after graph/forest copy, paste and history operations. |
| IDs, style and containment transfer safely | P33k checks graph mappings, atom properties, marks, explicit secondary-placement offsets, nested groups, paint order, internal self/cycle references and unchanged destination chemistry/placements. A real-backend style mismatch preserves document/revision/history. |
| Undo restores the prior state | P33k checks one-step Undo/Redo and mixed prior/new selection, original mapped IDs, containment/references and unchanged clipboard transport. Dedicated Cut checks cover frozen scope and history with scripted HTTP boundaries; P33k does not claim real-backend Cut integration. |
| Permission failures and representation losses are visible | Dedicated graph/graphic/MOL/SMILES/CDXML browser commands cover explicit activation/manual fallbacks and applicable consent/recovery paths. They use scripted browser clipboard and HTTP boundaries; these are UI/protocol checks, not successful OS permission or external-application interoperability evidence. |

The P33k harness replaces only browser clipboard transport. It supplies small
navigation/selection/history controls around production panels, so it does not
establish full editor canvas-selection/navigation behavior. The general smoke
exercises existing editor behavior separately. No clipboard check proves
scientific correctness, actual OS clipboard permission success or another
application's interpretation of exported plain text.

## Observed verification

The coordinating parent reported and retained the isolated run at the #172 head
in `/tmp/chemifree-p29d-verify-j4KjPa/p33k-full.log`: exit 0, no skipped standard
browser steps, peak anonymous memory 1,566 MiB, recorded 6,144 MiB cap and swap 0.
This host-local path is not a published artifact; the durable CI evidence is the
linked PR run. No result from a different head is substituted for these results.

Passed in that run:

- `npm run check`
- `npm run test:browser` (standard smoke, `skipped_steps=[]`)
- `npm run test:browser:clipboard`
- `npm run test:browser:mol-clipboard`
- `npm run test:browser:scene-clipboard`
- `npm run test:browser:smiles-clipboard`
- `npm run test:browser:cdxml-clipboard`
- `npm run test:browser:cdxml-import`
- `npm run test:browser:scene-cut`
- `npm run test:browser:clipboard-integration`

The linked #172 PR run has successful `check` and `browser` jobs. Post-merge main
CI and this completion documentation PR's reviews/CI are pending. Existing
conformance and requirement totals are retained without rerunning or inflating
them; browser outcomes are documented here rather than added as `check:` evidence
entries, because those entries must be invoked by `npm run check`.
