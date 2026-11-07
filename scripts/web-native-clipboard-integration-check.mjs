// Real clipboard panels → editor hook/client → HTTP API → application/domain.
// Only the browser clipboard transport is replaced. This does not establish OS
// clipboard permissions or compatibility with another drawing application.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

const root = fileURLToPath(new URL('../apps/web', import.meta.url))
const work = mkdtempSync(join(tmpdir(), 'chemifree-clipboard-integration-'))
let api
let server
let browser
let apiLog = ''
let stage = 'build'
let passed = false

// The harness supplies document navigation and explicit selection only. Production
// panels, hooks, validators, queue, history, autosave and HTTP client run unchanged.
function fixture(ids) {
  return `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { NativeClipboardPanel } from '/src/components/native-clipboard-panel.tsx'
import { SceneClipboardPanel } from '/src/components/scene-clipboard-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
const ids = ${JSON.stringify(ids)}
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open(ids.source) }, [])
  return <main>
    <nav aria-label="Test documents">{Object.entries(ids).map(([name, id]) =>
      <button key={id} disabled={editor.busy} onClick={() => void editor.open(id)}>Open {name}</button>)}</nav>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a3', 'b1']))}>Select source graph</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['s8', 's3', 's7']))}>Select source forest</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a1', 'b1', 's1']))}>Select destination context</button>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo edit</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo edit</button>
    <output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    <output aria-label="Current snapshot">{JSON.stringify(editor.current)}</output>
    <output aria-label="Current selection">{JSON.stringify([...editor.selection].sort())}</output>
    {editor.current && <div key={editor.current.document.id}>
      <NativeClipboardPanel document={editor.current.document} editor={editor} activePlacement={editor.current.document.id === ids.source ? 's2' : 's1'} />
      <SceneClipboardPanel document={editor.current.document} editor={editor} />
    </div>}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
}

/** Child readiness is observed from the bound address, rather than a timing sleep. */
async function startApi(path) {
  const env = { ...process.env, CHEMIFREE_DATA_DIR: work }
  delete env.CHEMIFREE_MODEL_DATA_ROOT
  delete env.CHEMIFREE_MODEL_TRUSTED_REGISTRY
  api = spawn(path, ['--port', '0'], { env, stdio: ['ignore', 'ignore', 'pipe'] })
  return new Promise((resolveReady, reject) => {
    const timer = setTimeout(() => reject(new Error(`API startup timed out: ${apiLog}`)), 15_000)
    api.once('error', (error) => { clearTimeout(timer); reject(error) })
    api.once('exit', (code) => { clearTimeout(timer); reject(new Error(`API exited (${code}): ${apiLog}`)) })
    api.stderr.on('data', (data) => {
      apiLog += data.toString()
      const match = apiLog.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) { clearTimeout(timer); resolveReady(match[1]) }
    })
  })
}

/** Session draft timestamps can advance independently; content and history cannot. */
function stable(snapshot) {
  return { document: snapshot.document, undo: snapshot.session.undo_depth, redo: snapshot.session.redo_depth }
}

function content(snapshot) {
  const { revision: _revision, ...document } = snapshot.document
  return document
}

try {
  execFileSync('cargo', ['build', '--locked', '--package', 'chemifree-api'], { stdio: 'inherit' })
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { encoding: 'utf8' }))
  stage = 'start API and seed real documents'
  const apiBase = await startApi(resolve(metadata.target_directory, 'debug', `chemifree-api${process.platform === 'win32' ? '.exe' : ''}`))
  const session = await (await fetch(`${apiBase}/api/v1/session`)).json()
  let sequence = 0
  async function request(path, body) {
    const response = await fetch(`${apiBase}/api/v1${path}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { 'content-type': 'application/json', [session.header]: session.token, origin: apiBase },
      body: body === undefined ? undefined : JSON.stringify({ request_id: `integration-${++sequence}`, ...body }),
      signal: AbortSignal.timeout(10_000),
    })
    const value = await response.json()
    assert.ok(response.ok, `${path}: ${response.status} ${JSON.stringify(value)}`)
    return value
  }
  async function seed(title, commands) {
    const created = await request('/documents', { title })
    const id = created.document.id
    await request(`/documents/${id}/transactions`, { expected_revision: 0, commands })
    // Clean seeds avoid source autosave/session changes obscuring copy immutability.
    await request(`/documents/${id}/save`, { expected_revision: 1 })
    return id
  }
  const graphic = (shape, points) => ({ op: 'add_graphic', shape, points })
  const source = await seed('Native clipboard integration source', [
    { op: 'add_atom', element: 'C', position: { x: 0, y: 0 }, hydrogens: { mode: 'specified', count: 1 }, radical: 'doublet', attachment_points: [1] },
    { op: 'add_atom', element: 'O', position: { x: 10, y: 0 } },
    { op: 'add_atom', element: 'N', position: { x: 10, y: 10 } },
    { op: 'add_atom', element: 'F', position: { x: 20, y: 10 } },
    { op: 'add_bond', begin: 'a1', end: 'a2', order: 'single' },
    { op: 'add_bond', begin: 'a2', end: 'a3', order: 'double' },
    { op: 'add_bond', begin: 'a3', end: 'a1', order: 'single' },
    { op: 'add_bond', begin: 'a3', end: 'a4', order: 'single' },
    { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'begin' } },
    { op: 'update_atom', atom: 'a1', isotope: 13, charge: 1 },
    { op: 'add_instance', source: 's1', offset: { x: 100, y: 20 } },
    graphic('line', [{ x: 1, y: -2 }, { x: 30, y: 4 }]),
    graphic('arrow', [{ x: 5, y: 6 }, { x: 7, y: 8 }]),
    graphic('rectangle', [{ x: 20, y: 25 }, { x: 10, y: 15 }]),
    { op: 'group', objects: ['s3', 's4'] },
    { op: 'group', objects: ['s6', 's5'] },
    graphic('line', [{ x: 40, y: 50 }, { x: 60, y: 70 }]),
    { op: 'add_reference', source: 's3', target: { kind: 'object', id: 's3' } },
    { op: 'add_reference', source: 's3', target: { kind: 'object', id: 's4' } },
    { op: 'add_reference', source: 's4', target: { kind: 'object', id: 's3' } },
    { op: 'add_reference', source: 's8', target: { kind: 'object', id: 's6' } },
  ])
  const destinationCommands = [
    { op: 'add_atom', element: 'C', position: { x: -30, y: -40 } },
    { op: 'add_atom', element: 'O', position: { x: 0, y: -40 }, bond_to: { atom: 'a1', order: 'double' } },
    { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'crossed' } },
    { op: 'set_bond_configuration', bond: 'b1', configuration: { kind: 'unknown' } },
  ]
  const graph = await seed('Native graph destination', destinationCommands)
  const scene = await seed('Graphic forest destination', [
    ...destinationCommands,
    { op: 'add_instance', source: 's1', offset: { x: 10, y: 20 } },
    { op: 'add_reference', source: 's1', target: { kind: 'atom', id: 'a1' } },
  ])
  const snapshot = (id) => request(`/documents/${id}`)
  const sourceBefore = await snapshot(source)
  const graphBefore = await snapshot(graph)
  const sceneBefore = await snapshot(scene)

  server = await createServer({
    root,
    configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
    server: { host: '127.0.0.1', port: 5173, strictPort: true, proxy: { '/api': apiBase } },
    plugins: [{
      name: 'real-backend-native-clipboard-harness',
      configureServer(vite) {
        vite.middlewares.use('/clipboard-integration', async (_request, response) => {
          const html = await vite.transformIndexHtml('/clipboard-integration', '<!doctype html><html><head><title>Real backend clipboard integration</title></head><body><div id="root"></div><script type="module" src="/clipboard-integration-harness.tsx"></script></body></html>')
          response.setHeader('content-type', 'text/html')
          response.end(html)
        })
      },
      resolveId(id) { if (id === '/clipboard-integration-harness.tsx') return id },
      load(id) { if (id === '/clipboard-integration-harness.tsx') return fixture({ source, graph, scene }) },
    }],
  })
  await server.listen()
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } })
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.addInitScript(() => {
    window.clipboardTransport = { text: '', writes: [], reads: [] }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText(text) {
        window.clipboardTransport.writes.push({ text, activated: navigator.userActivation.isActive })
        window.clipboardTransport.text = text
        return Promise.resolve()
      },
      readText() {
        window.clipboardTransport.reads.push({ activated: navigator.userActivation.isActive })
        return Promise.resolve(window.clipboardTransport.text)
      },
    } })
  })
  const button = (name) => page.getByRole('button', { name, exact: true })
  const shown = async () => JSON.parse(await page.getByLabel('Current snapshot').innerText())
  async function open(name, id) {
    await button(`Open ${name}`).click()
    await page.waitForFunction((expected) => document.querySelector('[aria-label="Current document"]')?.textContent === expected, id)
  }
  async function revision(expected) {
    await page.waitForFunction((value) => JSON.parse(document.querySelector('[aria-label="Current snapshot"]').textContent)?.document.revision === value, expected)
  }
  // Observe real responses; there are deliberately no page.route/fulfill handlers.
  async function operation(name, action, outcome = 'ok') {
    const pending = page.waitForResponse((response) => response.url().endsWith('/api/v1/operations') && response.request().postDataJSON()?.operation === name)
    await action()
    const response = await pending
    const body = await response.json()
    assert.equal(body.operation, name)
    assert.equal(body.outcome, outcome, JSON.stringify(body))
    if (outcome === 'ok') assert.deepEqual(body.loss_report, [])
    return body
  }
  async function copy(kind) {
    const graphMode = kind === 'graph'
    const envelope = await operation(graphMode ? 'document.clipboard_copy' : 'document.clipboard_scene_copy', () => button(graphMode ? 'Copy fragment…' : 'Copy graphics…').click())
    const text = page.getByRole('textbox', { name: graphMode ? 'Native fragment text' : 'Native graphics text', exact: true })
    await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
    assert.equal(await text.inputValue(), envelope.result.payload)
    await button('Copy to clipboard').click()
    await page.getByRole('status').filter({ hasText: graphMode ? 'Native fragment text copied to clipboard.' : 'Native graphics text copied to clipboard.' }).waitFor()
    await button('Close').click()
    return envelope.result
  }
  async function paste(kind, payload) {
    const graphMode = kind === 'graph'
    await button(graphMode ? 'Paste fragment…' : 'Paste graphics…').click()
    const text = page.getByRole('textbox', { name: graphMode ? 'Native fragment text' : 'Native graphics text', exact: true })
    assert.equal(await text.inputValue(), '', 'opening Paste must not read clipboard')
    await button('Read clipboard').click()
    await page.waitForFunction((expected) => document.querySelector('textarea')?.value === expected, payload)
    await page.getByRole('textbox', { name: 'Offset x (pt)', exact: true }).fill('30')
    await page.getByRole('textbox', { name: 'Offset y (pt)', exact: true }).fill('40')
    const envelope = await operation(graphMode ? 'document.clipboard_paste' : 'document.clipboard_scene_paste', () => button(graphMode ? 'Paste fragment' : 'Paste graphics').click())
    await page.getByRole('dialog').waitFor({ state: 'detached' })
    return envelope.result
  }
  async function history(before, after, selectedBefore, selectedAfter) {
    await button('Undo edit').click()
    await revision(after.document.revision + 1)
    const undone = await shown()
    assert.deepEqual(content(undone), { ...content(before), selection: selectedBefore })
    assert.equal(undone.session.undo_depth, before.session.undo_depth)
    assert.equal(undone.session.redo_depth, 1)
    assert.deepEqual(JSON.parse(await page.getByLabel('Current selection').innerText()), [...selectedBefore.objects, ...selectedBefore.atoms, ...selectedBefore.bonds].sort())
    await button('Redo edit').click()
    await revision(after.document.revision + 2)
    assert.deepEqual(content(await shown()), content(after), 'Redo must reuse the original IDs and complete contents')
    assert.equal((await shown()).session.undo_depth, before.session.undo_depth + 1)
    assert.deepEqual(JSON.parse(await page.getByLabel('Current selection').innerText()), selectedAfter.sort())
  }

  stage = 'native graph cross-document copy and paste'
  await page.goto('http://127.0.0.1:5173/clipboard-integration')
  await page.waitForFunction((id) => document.querySelector('[aria-label="Current document"]')?.textContent === id, source)
  await button('Select source graph').click()
  const graphCopy = await copy('graph')
  assert.deepEqual(graphCopy.copied, { objects: [], atoms: ['a1', 'a2', 'a3'], bonds: ['b1', 'b2', 'b3'] })
  assert.deepEqual(graphCopy.boundary_bonds, ['b4'])
  assert.equal(graphCopy.instance, 's2')
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore), 'copy must be read-only')
  await open('graph', graph)
  await button('Select destination context').click()
  const graphPaste = await paste('graph', graphCopy.payload)
  assert.deepEqual(graphPaste.mapping, {
    atoms: [{ source: 'a1', target: 'a3' }, { source: 'a2', target: 'a4' }, { source: 'a3', target: 'a5' }],
    bonds: [{ source: 'b1', target: 'b2' }, { source: 'b2', target: 'b3' }, { source: 'b3', target: 'b4' }],
  })
  const graphAfter = await shown()
  assert.equal(graphAfter.document.revision, graphBefore.document.revision + 1)
  assert.equal(graphAfter.session.undo_depth, graphBefore.session.undo_depth + 1)
  assert.deepEqual(graphAfter.document.atoms.slice(0, 2), graphBefore.document.atoms)
  assert.deepEqual(graphAfter.document.bonds.slice(0, 1), graphBefore.document.bonds)
  assert.deepEqual(graphAfter.document.atoms.slice(2), [
    { ...sourceBefore.document.atoms[0], id: 'a3', position: { x: 130, y: 60 } },
    { ...sourceBefore.document.atoms[1], id: 'a4', position: { x: 140, y: 60 } },
    { ...sourceBefore.document.atoms[2], id: 'a5', position: { x: 140, y: 70 } },
  ], 'isotope, charge, specified H, radical and attachment state survive with explicit source placement + offset')
  assert.deepEqual({
    isotope: graphAfter.document.atoms[2].isotope,
    charge: graphAfter.document.atoms[2].charge,
    hydrogens: graphAfter.document.atoms[2].hydrogens,
    radical: graphAfter.document.atoms[2].radical,
    attachment_points: graphAfter.document.atoms[2].attachment_points,
  }, { isotope: 13, charge: 1, hydrogens: { mode: 'specified', count: 1 }, radical: 'doublet', attachment_points: [1] })
  assert.deepEqual(graphAfter.document.bonds.slice(1), [
    { id: 'b2', begin: 'a3', end: 'a4', order: 'single', query: null, mark: { kind: 'wedge', narrow: 'begin' } },
    { id: 'b3', begin: 'a4', end: 'a5', order: 'double', query: null, mark: null },
    { id: 'b4', begin: 'a5', end: 'a3', order: 'single', query: null, mark: null },
  ])
  assert.deepEqual(graphAfter.document.stereo, graphBefore.document.stereo)
  assert.deepEqual(graphAfter.document.settings, graphBefore.document.settings)
  assert.deepEqual(graphAfter.document.scene.instances[0].positions.slice(0, 2), graphBefore.document.scene.instances[0].positions)
  assert.deepEqual(graphAfter.document.scene.instances[0].positions.slice(2), [
    { atom: 'a3', x: 130, y: 60 }, { atom: 'a4', x: 140, y: 60 }, { atom: 'a5', x: 140, y: 70 },
  ])
  assert.deepEqual(graphAfter.document.scene.objects, graphBefore.document.scene.objects)
  assert.deepEqual(graphAfter.document.scene.roots, graphBefore.document.scene.roots)
  assert.deepEqual(graphAfter.document.scene.references, graphBefore.document.scene.references)
  assert.deepEqual(graphAfter.document.selection, { objects: [], atoms: ['a3', 'a4', 'a5'], bonds: ['b2', 'b3', 'b4'] })
  await history(graphBefore, graphAfter, { objects: ['s1'], atoms: ['a1'], bonds: ['b1'] }, ['a3', 'a4', 'a5', 'b2', 'b3', 'b4'])
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore), 'destination paste/history cannot mutate source')

  stage = 'real backend rejection preserves graph and history'
  const beforeRefusal = await snapshot(graph)
  const mismatched = JSON.parse(graphCopy.payload)
  mismatched.settings.bond_length_pt = 31
  await button('Paste fragment…').click()
  await page.getByRole('textbox', { name: 'Native fragment text', exact: true }).fill(JSON.stringify(mismatched))
  const refused = await operation('document.clipboard_paste', () => button('Paste fragment').click(), 'error')
  assert.equal(refused.diagnostics[0].code, 'invalid_command')
  assert.equal(refused.diagnostics[0].details.reason, 'clipboard_style_mismatch')
  await page.getByRole('alert').waitFor()
  assert.deepEqual(stable(await snapshot(graph)), stable(beforeRefusal))
  assert.deepEqual(stable(await shown()), stable(beforeRefusal))
  await button('Close').click()

  stage = 'nested graphic forest cross-document copy and paste'
  await open('source', source)
  await button('Select source forest').click()
  const sceneCopy = await copy('scene')
  assert.deepEqual(sceneCopy.roots, ['s7', 's8'])
  assert.deepEqual(sceneCopy.copied, { objects: ['s7', 's6', 's3', 's4', 's5', 's8'], atoms: [], bonds: [] })
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore))
  await open('scene', scene)
  await button('Select destination context').click()
  const scenePaste = await paste('scene', sceneCopy.payload)
  assert.deepEqual(scenePaste.mapping.objects.map((item) => item.source), ['s7', 's6', 's3', 's4', 's5', 's8'])
  const mapping = Object.fromEntries(scenePaste.mapping.objects.map(({ source: from, target }) => [from, target]))
  assert.equal(new Set(Object.values(mapping)).size, 6)
  assert.ok(Object.values(mapping).every((id) => !sceneBefore.document.scene.objects.some((object) => object.id === id)), 'all pasted IDs must be fresh in the destination')
  const sceneAfter = await shown()
  assert.equal(sceneAfter.document.scene.objects.length, 8, 'exactly six fresh forest objects append to the two existing placements')
  assert.equal(sceneAfter.document.revision, sceneBefore.document.revision + 1)
  assert.equal(sceneAfter.session.undo_depth, sceneBefore.session.undo_depth + 1)
  assert.deepEqual(sceneAfter.document.atoms, sceneBefore.document.atoms)
  assert.deepEqual(sceneAfter.document.bonds, sceneBefore.document.bonds)
  assert.deepEqual(sceneAfter.document.stereo, sceneBefore.document.stereo)
  assert.deepEqual(sceneAfter.document.settings, sceneBefore.document.settings)
  assert.deepEqual(sceneAfter.document.scene.instances, sceneBefore.document.scene.instances)
  assert.deepEqual(sceneAfter.document.scene.pages, sceneBefore.document.scene.pages)
  const object = (id) => sceneAfter.document.scene.objects.find((item) => item.id === id)
  assert.deepEqual(object('s1'), sceneBefore.document.scene.objects[0])
  assert.deepEqual(object('s2'), sceneBefore.document.scene.objects[1])
  assert.deepEqual(sceneAfter.document.scene.roots, ['s1', 's2', mapping.s7, mapping.s8])
  assert.deepEqual(object(mapping.s7), { kind: 'group', id: mapping.s7, parent: null, children: [mapping.s6, mapping.s5] })
  assert.deepEqual(object(mapping.s6), { kind: 'group', id: mapping.s6, parent: mapping.s7, children: [mapping.s3, mapping.s4] })
  assert.deepEqual(object(mapping.s3), { kind: 'graphic', line_width_pt: null, id: mapping.s3, parent: mapping.s6, shape: 'line', points: [{ x: 31, y: 38 }, { x: 60, y: 44 }] })
  assert.deepEqual(object(mapping.s4), { kind: 'graphic', line_width_pt: null, id: mapping.s4, parent: mapping.s6, shape: 'arrow', points: [{ x: 35, y: 46 }, { x: 37, y: 48 }] })
  assert.deepEqual(object(mapping.s5), { kind: 'graphic', line_width_pt: null, id: mapping.s5, parent: mapping.s7, shape: 'rectangle', points: [{ x: 50, y: 65 }, { x: 40, y: 55 }] })
  assert.deepEqual(object(mapping.s8), { kind: 'graphic', line_width_pt: null, id: mapping.s8, parent: null, shape: 'line', points: [{ x: 70, y: 90 }, { x: 90, y: 110 }] })
  // References are canonicalized by destination IDs, independently of payload order.
  const referenceKeys = (references) => references.map(({ source, target }) => `${source}:${target.kind}:${target.id}`).sort()
  assert.deepEqual(referenceKeys(sceneAfter.document.scene.references), referenceKeys([
    { source: 's1', target: { kind: 'atom', id: 'a1' } },
    { source: mapping.s3, target: { kind: 'object', id: mapping.s3 } },
    { source: mapping.s3, target: { kind: 'object', id: mapping.s4 } },
    { source: mapping.s4, target: { kind: 'object', id: mapping.s3 } },
    { source: mapping.s8, target: { kind: 'object', id: mapping.s6 } },
  ]))
  assert.deepEqual(sceneAfter.document.selection, { objects: [mapping.s7, mapping.s8].sort(), atoms: [], bonds: [] })
  await history(sceneBefore, sceneAfter, { objects: ['s1'], atoms: ['a1'], bonds: ['b1'] }, [mapping.s7, mapping.s8])
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore))
  assert.deepEqual(stable(await snapshot(graph)), stable(beforeRefusal), 'forest paste cannot change the other destination')
  const transport = await page.evaluate(() => window.clipboardTransport)
  assert.deepEqual(transport.writes, [{ text: graphCopy.payload, activated: true }, { text: sceneCopy.payload, activated: true }])
  assert.deepEqual(transport.reads, [{ activated: true }, { activated: true }], 'history never reads or writes clipboard')
  assert.deepEqual(errors, [])
  passed = true
  console.log('Real backend native graph and graphic forest cross-document clipboard integration passed.')
} catch (error) {
  console.error(`Native clipboard integration failed at ${stage}. Data retained at ${work}.\n${apiLog}`)
  throw error
} finally {
  await browser?.close()
  await server?.close()
  if (api?.pid && api.exitCode === null && api.signalCode === null) {
    const exited = once(api, 'exit')
    api.kill('SIGTERM')
    const force = setTimeout(() => api.kill('SIGKILL'), 5_000)
    await exited
    clearTimeout(force)
  }
  if (passed) rmSync(work, { recursive: true, force: true })
}
