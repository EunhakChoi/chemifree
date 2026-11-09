# Web SVG download panel: verification record

Scope: the web editor gets an **SVG drawing export** panel next to the CDXML/CDX panel: preview, consent listing, **Download SVG** (`chemifree.svg`, `image/svg+xml`). No service change. `DocumentClient.exportSvg` reads the closed result shape strictly and ignores unknown fields; `useConsentedExport` (new hook) holds the consent, stale-drawing and download-recheck logic that the CDXML/CDX panel used to hold, and both panels use it; the editor's stale guard is one helper for `exportInterchange` and `exportSvg`. Not included: an SVG rendering in the page, copy to the clipboard, a desktop control. Bounded export as in the CONTRACT (P38a to P38b2); not general SVG output.

## Tests

- `scripts/web-interchange-export-check.mjs`: 21 new `svg export ...` cases (request shape before and after consent, the returned object, unknown fields at three levels ignored and not copied, the empty `0 0 0 0` view box, consent refusal and `loss_consent_changed`, eight malformed-reply refusals, malformed digests refused before any request). The 27 earlier interchange cases pass unchanged.
- `scripts/browser-smoke.mjs`: one new block (plain drawing and download, stored configuration plus group with consent, stale consent digest, stale snapshot held across an edit). The existing CDXML/CDX interchange block, the import blocks and `npm run test:browser:cdxml-clipboard` (the refactored shipped panel) pass in Chromium. The full smoke runs in CI; the sandbox ran trimmed copies of the script (the smoke has no step filter).

## Mutations (each in a private copy)

| Mutation | Caught by |
| --- | --- |
| `media_type` check dropped | the media-type refusal case |
| view-box finiteness and sign checks dropped | the width -1, NaN and missing-y cases |
| closed key list on the result | the unknown-result-field case |
| `result` spread into the returned object | the unknown-result-field case |
| `accepted_losses` sent as `null` on the first request | the request-shape case |
| download re-export replaced by the prepared result | the stale-digest browser case (the file downloads) |
| both stale-snapshot layers removed (hook recheck and editor guard) | the held-response browser case |

Not killed, by design: `accepted_losses` key present with `undefined` (an equivalent mutant: `JSON.stringify` drops it, replaced by the `null` variant above); either stale-snapshot layer removed alone (the layers are redundant on purpose, both together are proven).

## Decisions and notes

- The logic is shared through a hook, not by adding `svg` to the CDXML panel, whose note describes one interchange molecule and would be untrue for a picture.
- The CDXML panel keeps its copy-status reset in a local wrapper around the hook's `preview`; the hook's own download-restart path does not clear it, which is not visible because the restart clears the prepared export first.
- The panel note does not name every consent loss (for example `stereo_mark`, `query_label_truncated`); the consent screen shows the real list.
- Sandbox observations: workers saw an `@eslint/js` nesting gap (`apps/web/node_modules`) that fails lint in a bare sandbox without an install, and Vite needed a writable `apps/web/node_modules` for the smoke; neither affects the repository or CI.
