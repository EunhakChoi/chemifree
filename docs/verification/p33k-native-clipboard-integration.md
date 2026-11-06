# P33k real-backend native clipboard integration

Status: authored, not executed. P33 remains `in_progress`.

`npm run test:browser:clipboard-integration` builds `chemifree-api` with the
lockfile, launches it on an OS-assigned loopback port with disposable storage,
then starts Vite on port 5173 and Chromium. Port 5173 must be free. Browser setup
is the same as the normal smoke: install the locked Playwright Chromium or set
`CHEMIFREE_BROWSER` to an executable. The CI `browser` job runs this command.

The check drives production `NativeClipboardPanel`, `SceneClipboardPanel`,
`useDocumentEditor` and `DocumentClient`. A small React harness supplies explicit
document navigation, selection, and history buttons. Real HTTP transactions seed
three documents. No HTTP responses or domain/application modules are mocked.
The browser's clipboard transport is replaced with a plain-text buffer that
records activation. This is an integration check of the clipboard panels and
backend; it does not exercise the full editor's canvas selection or navigation,
OS clipboard permission handling, another drawing application, external formats,
or scientific correctness.

The existing isolated panel checks cover permission failures, uncertain request
replay, restart reconciliation and queue capture. This check fills the separate
real-backend transfer gap; it does not replace those checks or the general smoke.

Decisions and assertions:

- Graph source selection combines an atom with selected-bond endpoints, inducing
  a triangle and reporting a crossing bond. The explicit second placement supplies
  coordinates. The destination already contains atoms, a marked/configured bond
  and a placement, so ID allocation and existing-content preservation are observable.
  Expected mapped endpoints, bond orders, wedge, isotope, charge, specified
  hydrogen, radical, attachment points and offset positions are asserted.
- Graphic selection deliberately includes a descendant of a selected nested
  group and a second root in reversed selection order. Copy must deduplicate the
  closure and preserve scene paint order. Paste must append exactly six fresh
  objects, map both containment levels and all internal references, including a
  self-link and cycle, and preserve rectangle corner order. Two destination
  molecule placements, existing chemistry, stereo, style and references stay intact.
- Copy leaves source content, revision and history unchanged. Destination paste
  adds exactly one revision/history entry. One Undo restores the complete prior
  contents and mixed selection; Redo restores the same mapped IDs and new
  selection. Source and the unrelated destination remain unchanged throughout.
- Submitting native graph text with a valid but mismatched bond-length setting
  must reach the real backend and return `clipboard_style_mismatch`, leaving
  content, revision and Undo/Redo depths unchanged.
- Opening Paste begins with empty text. Explicit browser write/read buttons must
  carry user activation. The complete flow has exactly two writes and two reads;
  history and the refusal do not touch clipboard transport.

Autosave may independently change draft timestamps. Immutability comparisons
therefore include the entire document and Undo/Redo depths while excluding draft
timestamp/session bookkeeping. Source seeds are saved before browser navigation.
Waiting uses rendered revisions, dialog completion, real responses and process
readiness rather than arbitrary sleeps. Passing runs remove disposable storage;
failed runs retain it and log the failing stage and API output. Startup timeout
and bounded process termination protect cleanup.

No tests, builds, browsers or deliberate mutations were run by this implementer,
as instructed by the parent coordinating shared isolated validation. The required
next evidence is the new browser command, the normal project checks, and a scoped
mutation run (for example, remove native paste offset application or internal
reference mapping and observe the relevant assertion fail, then restore it).
Neither review completion nor successful CI is claimed here. The PR's completed
Codex review and fresh-context independent review, their reviewed SHA, timestamps
and finding dispositions must be recorded through the normal repository workflow.
