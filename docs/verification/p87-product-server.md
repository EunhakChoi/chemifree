# P87 product server verification

Date: 2026-11-14

Scope: the local product path. `chemifree-api` serves the built web app on its own loopback
origin, the browser saves a document through the UI, the CLI reads it from the same data
directory, and the server lifecycle rules hold. This is a local Linux check, also run by the CI browser job with debug binaries. It does not
claim scientific behavior.

## What the check runs

`node scripts/product-server-check.mjs [--profile release|debug]`, default `release`. The check
builds nothing. It needs the binaries of the profile and `apps/web/dist/index.html`, and it finds
the binaries with `cargo metadata`, so `CARGO_TARGET_DIR` is respected. Servers listen on
127.0.0.1 with an OS-chosen port.

It runs seven checks in order and prints `ok <name>` for each, then
`product-server-check: all 7 checks passed (<profile>)`. The first failure prints
`FAILED <name>: <message>` and the most recent server's standard error, then exits 1. An
interrupt (SIGINT or SIGTERM) prints the same failure line and exits 130 or 143. Every run,
including an interrupt, kills the servers it started, closes the browser and removes its
temporary directory. A SIGKILL of the check itself cannot run this cleanup.

- `headers`: `GET /`, the hashed script that `index.html` names, and `GET /api/v1/health` carry
  the server's Content-Security-Policy, `X-Content-Type-Options: nosniff`,
  `Referrer-Policy: no-referrer` and `Cross-Origin-Opener-Policy: same-origin`. `GET /` is
  `no-cache` and the hashed script is immutable. Why: the web app runs only under this policy,
  so a missing or weakened header is a regression of the security baseline.
- `ui-save`: Chromium opens the app, creates a document from the UI, draws a bond on the canvas,
  waits for the save badge to read "Unsaved changes", clicks Save and waits for "Saved". Why: the
  save goes through the server that the CLI reads, and the UI's own badge shows the edit was
  unsaved before Save.
- `cli-read`: `chemifree documents show <id> --json` on the same data directory exits 0 and
  returns the same document with two atoms and one bond. Why: the CLI and the web app share one
  local document store.
- `second-server`: a second `chemifree-api` on the same data directory exits 4 and its message
  says the data directory is already used by another Chemifree API server. Why: the
  cross-process lock keeps a second writer out.
- `port-conflict`: a server asked for the port in use exits 3 and its message says the port is
  already in use. Why: a port conflict is an error, not a second listener.
- `sigkill-restart`: SIGKILL the server, restart it on the same data directory, and the document
  is listed. Why: the operating system releases the lock when a process dies, so a crash does not
  block the next start.
- `sigterm-recovery`: in the browser, open the document and draw a bond without saving. The
  server must hold the unsaved edit. SIGTERM must exit 0 and leave a draft file. After restart,
  the document list reads "Unsaved work found" and the recovery notice is `recoverable`. Why: a
  clean stop keeps unsaved work recoverable. This covers the final draft that SIGTERM writes, not
  the timing of autosave drafts.

Each browser page must report no console error, uncaught page error, crash or Content-Security-
Policy violation. Nothing is filtered.

## How to run

Local release, the profile that `npm start` uses:

```sh
npm run build:product
npm run test:product
```

Chromium: run `npx playwright-core install chromium`, or set `CHEMIFREE_BROWSER` to a Chrome or
Chromium executable, as `scripts/browser-smoke.mjs` does. The check needs `cargo` on the PATH.

CI form, debug profile. The browser job should not compile release binaries, so it uses the debug
profile, which runs the same checks:

```sh
npm run build:web
cargo build --locked --package chemifree-api --package chemifree-cli
npm run test:product -- --profile debug
```

The CI `browser` job runs these commands (with Playwright's bundled Chromium).

## Results on 2026-11-14

- Source: base `1c88fa2c` plus uncommitted worktree changes to `docs/CONTRACT.md`,
  `package.json`, `scripts/product-server-check.mjs` and this file. These results do not come
  from a commit.
- Node v22.22.2, playwright-core 1.56.1, Chromium 157.0.8103.38 (Google Chrome from
  `/opt/google/chrome`, through `CHEMIFREE_BROWSER`).
- Both runs executed inside the sandbox runner, with cargo and the check inside it, using the
  shared build cache.

Release, after `npm run build:product` (cargo reported the release build up to date, 0.04 s):

```
product-server-check (release): node v22.22.2, chromium 157.0.8103.38
ok headers
ok ui-save
ok cli-read
ok second-server
ok port-conflict
ok sigkill-restart
ok sigterm-recovery
product-server-check: all 7 checks passed (release)
```

Check wall time: 2 s (1 s resolution). No `chemifree-api` or `chrome` process and no
`chemifree-product-*` directory remained afterwards.

Debug, after `npm run build:web` and the debug `cargo build`: the same seven lines with `(debug)`.
The first debug build of this session took 19.65 s; the final run found it up to date. Check wall
time: 2 s. No leftovers remained.

Negative tests. Each ran in a copy inside the sandbox, so the repository file was not changed.
Every case ended with no `chemifree-api` or `chrome` process and no temporary directory left.

- Expected CSP changed to `frame-ancestors 'self'`: `FAILED headers: GET / content-security-policy
  header is "...frame-ancestors 'none'...", expected "...frame-ancestors 'self'..."`, exit 1.
- Expected recovery state changed to `outdated`: `FAILED sigterm-recovery: the recovery notice
  state is "recoverable", expected "outdated"`, exit 1.
- SIGINT during a live check (a 5 s delay inserted before `headers`, signal at 4 s):
  `FAILED headers: interrupted by SIGINT`, exit 130.
- SIGTERM during the same live check: `FAILED headers: terminated by SIGTERM`, exit 143.
- SIGINT while Chromium is still launching (a wrapper delays the launch by 3 s, signal at 1 s):
  `FAILED setup: interrupted by SIGINT`, exit 130. The run continued after the launch and started
  no server.

## Defects found and fixed in the check

These were faults in the check, not in the product. The product code was not changed.

- The first run failed in `sigterm-recovery`. After the Open click, the page had scrolled the
  document list, so the canvas sat above the viewport and the check clicked outside the page.
  Diagnosis: `elementFromPoint` returned nothing at the click point. After the canvas was scrolled
  into view, the same click produced "Unsaved changes" and the server held the edit. Fixed: the
  canvas is clicked as a locator action, which scrolls it into view and fails if another element
  covers the point.
- Playwright's default SIGINT handler calls `process.exit(130)` once its browsers close. That can
  end the check before its cleanup removes the work directory, which the SIGINT negative test
  showed. Fixed: the check launches Chromium with `handleSIGHUP`, `handleSIGINT` and
  `handleSIGTERM` set to false and handles signals itself.
- Cleanup problems were dropped on the signal path. Fixed: they are printed on every path.
- A process started after cleanup had begun could outlive the run. Fixed: starting a process is
  refused once cleanup has begun.

## Known limits

- Linux only, on a WSL2 kernel. Windows and macOS are untested.
- The release path was run locally; CI runs the debug form.
- The local runs used Google Chrome; the CI browser job uses Playwright's bundled Chromium.
- The check tests the binaries and `apps/web/dist` that exist; it does not check that they are
  current. Run `npm run build:product` (or the debug build) after changing sources.
- Recovery: after the page closes, one more edit goes through the API and SIGTERM follows at
  once; the draft must then hold that edit's revision and counts, which neither the web autosave
  (page closed) nor the server's 2 s quiet draft can have written first. This is a timing bound,
  not a hard proof; if the draft already holds the edit before the stop, the check prints a note
  and checks only that the draft exists. After SIGKILL it checks only the restart, not recovery.
- Not packaged: no single binary (P88) and no installer.
- `scripts/product-server-check.mjs` is covered by `node --check` and review. No repository lint
  gate covers `.mjs` files.
