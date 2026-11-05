// Fixed viewport examples and screen-sized picking thresholds, independent of DOM rendering.
import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const {
  MIN_SCALE, MAX_SCALE, clampScale, fitViewport, panViewport, zoomViewport,
  clientToWorld, worldToClient, svgViewBox,
} = await import('../apps/web/src/editor/viewport.ts')
const { hitTest, atomIndex, bondUnderlay, bondStrokes, graphicUnderlay } = await import('../apps/web/src/editor/geometry.ts')
const { DEFAULT_SETTINGS } = await import('../apps/web/src/lib/document-contract.ts')

const size = { width: 800, height: 400 }
const view = { center: { x: 100, y: 50 }, scale: 2 }

function closePoint(actual, expected) {
  assert.ok(Math.abs(actual.x - expected.x) < 1e-9, `x: ${actual.x} != ${expected.x}`)
  assert.ok(Math.abs(actual.y - expected.y) < 1e-9, `y: ${actual.y} != ${expected.y}`)
}

for (const [scale, thin, wide] of [[0.25, 32, 32], [1, 8, 10], [32, 1.3875, 4.1875]]) {
  test(`graphic outlines and arrowheads retain a visible screen halo at scale ${scale}`, () => {
    assert.equal(graphicUnderlay(1.2, 1 / scale), thin)
    assert.equal(graphicUnderlay(4, 1 / scale), wide)
    assert.ok((thin - 1.2) * scale >= 6, 'The selected outline extends at least 3 CSS pixels beyond each side of the ink.')
    assert.ok((wide - 4) * scale >= 6, 'A wide foreground cannot hide its selected outline.')
  })
}

test('graphic selection keeps the established 1x appearance for callers without a viewport', () => {
  assert.equal(graphicUnderlay(1.2), 8)
  assert.equal(graphicUnderlay(4), 10)
})

for (const [scale, singleWidth, tripleWidth] of [
  [0.25, 32, 32],
  [4, 2.7, 9.5],
  [32, 1.3875, 8.1875],
]) {
  test(`bond highlights include visible ink plus a screen-sized halo at scale ${scale}`, () => {
    assert.equal(bondUnderlay('single', 1.2, 1, 1 / scale), singleWidth)
    assert.equal(bondUnderlay('triple', 1.2, 1, 1 / scale), tripleWidth)
  })
}

for (const [order, offset] of [['double', 2.2], ['triple', 3.4]]) {
  test(`high-zoom picking includes visible ${order} strokes on both sides of the axis`, () => {
    const document = { ...bonded, atoms: [atom('a1', 0), atom('a2', 30)], bonds: [{ ...bonded.bonds[0], order }] }
    const index = atomIndex(document)
    for (const y of [-offset, offset, 0]) {
      assert.deepEqual(hitTest(document, { x: 15, y }, new Set(), index, new Map(), 0.25), { kind: 'bond', id: 'b1' })
    }
    assert.equal(hitTest(document, { x: 15, y: offset + 1.3 }, new Set(), index, new Map(), 0.25), null)
    const wider = { ...document, settings: { ...DEFAULT_SETTINGS, line_width_pt: 2.4 } }
    assert.deepEqual(hitTest(wider, { x: 15, y: 2 * offset }, new Set(), index, new Map(), 0.25), { kind: 'bond', id: 'b1' })
  })
}

for (const [mark, order, ink, outside, underlay] of [
  [{ kind: 'wedge', narrow: 'begin' }, 'single', { x: 27, y: 1.7 }, { x: 27, y: 2.7 }, 4.1875],
  [{ kind: 'wedge', narrow: 'end' }, 'single', { x: 3, y: 1.7 }, { x: 3, y: 2.7 }, 4.1875],
  [{ kind: 'hash', narrow: 'begin' }, 'single', { x: 28.35, y: 1.7 }, { x: 28.35, y: 2.7 }, 4.1875],
  [{ kind: 'hash', narrow: 'end' }, 'single', { x: 1.65, y: 1.7 }, { x: 1.65, y: 2.7 }, 4.1875],
  [{ kind: 'wavy' }, 'single', { x: 9.375, y: 1.8 }, { x: 9.375, y: 3 }, 4.9875],
  [{ kind: 'crossed' }, 'double', { x: 7.5, y: 1.1 }, { x: 7.5, y: 3 }, 5.7875],
]) {
  test(`32x picking and highlighting follow ${mark.kind} ${mark.narrow ?? ''} ink`, () => {
    const document = { ...bonded, atoms: [atom('a1', 0), atom('a2', 30)], bonds: [{ ...bonded.bonds[0], order, mark }] }
    const index = atomIndex(document)
    assert.deepEqual(hitTest(document, ink, new Set(), index, new Map(), 1 / 32), { kind: 'bond', id: 'b1' })
    const strokes = new Map([['b1', bondStrokes(document.bonds[0], { x: 0, y: 0 }, { x: 30, y: 0 }, null, null)]])
    assert.deepEqual(hitTest(document, ink, new Set(), index, new Map(), 1 / 32, strokes), { kind: 'bond', id: 'b1' })
    assert.equal(hitTest(document, outside, new Set(), index, new Map(), 1 / 32), null)
    assert.ok(Math.abs(bondUnderlay(order, 1.2, 1, 1 / 32, mark) - underlay) < 1e-12)
  })
}

test('picking a marked single bond does not enlarge every ordinary single bond', () => {
  const document = { ...bonded, atoms: [atom('a1', 0), atom('a2', 30)] }
  assert.equal(hitTest(document, { x: 15, y: 1.7 }, new Set(), atomIndex(document), new Map(), 1 / 32), null)
})

test('a 2px/pt viewport maps its center and offset using CSS pixels', () => {
  assert.deepEqual(worldToClient(view, { x: 100, y: 50 }, size), { x: 400, y: 200 })
  assert.deepEqual(worldToClient(view, { x: 130, y: 40 }, size), { x: 460, y: 180 })
  assert.deepEqual(clientToWorld(view, { x: 460, y: 180 }, size), { x: 130, y: 40 })
  assert.deepEqual(svgViewBox(view, size), { x: -100, y: -50, width: 400, height: 200 })
})

test('pan moves the drawing with the pointer and scales pixel travel into document points', () => {
  assert.deepEqual(panViewport(view, { x: 80, y: -40 }), { center: { x: 60, y: 70 }, scale: 2 })
  assert.deepEqual(panViewport({ center: { x: 0, y: 0 }, scale: 0.5 }, { x: 80, y: -40 }),
    { center: { x: -160, y: 80 }, scale: 0.5 })
})

test('zoom keeps an off-center pointer anchor at the same document point', () => {
  const result = zoomViewport(view, 2, { x: 600, y: 100 }, size)
  assert.deepEqual(result, { center: { x: 150, y: 25 }, scale: 4 })
  assert.deepEqual(clientToWorld(result, { x: 600, y: 100 }, size), { x: 200, y: 0 })
})

test('zoom clamping uses the effective scale when preserving its anchor', () => {
  assert.equal(MIN_SCALE, 1 / 4096)
  assert.equal(MAX_SCALE, 32)
  assert.equal(clampScale(1e-8), 1 / 4096)
  assert.equal(clampScale(1e6), 32)
  assert.deepEqual(zoomViewport({ center: { x: 0, y: 0 }, scale: 16 }, 4, { x: 600, y: 100 }, size),
    { center: { x: 6.25, y: -3.125 }, scale: 32 })
})

for (const scale of [1 / 4096, 0.25, 1, 2, 32]) {
  test(`world/client transforms roundtrip positive and negative positions at scale ${scale}`, () => {
    const viewport = { center: { x: -123.5, y: 74.25 }, scale }
    const world = { x: 812.125, y: -400.5 }
    const client = { x: 23.75, y: 333.125 }
    closePoint(clientToWorld(viewport, worldToClient(viewport, world, size), size), world)
    closePoint(worldToClient(viewport, clientToWorld(viewport, client, size), size), client)
  })
}

test('fit centers explicit bounds and respects the measured narrow viewport', () => {
  const bounds = { x: -100, y: -50, width: 400, height: 100 }
  assert.deepEqual(fitViewport(bounds, { width: 800, height: 400 }), { center: { x: 100, y: 0 }, scale: 2 })
  assert.deepEqual(fitViewport(bounds, { width: 200, height: 400 }), { center: { x: 100, y: 0 }, scale: 0.5 })
})

test('an unmeasured viewport does not manufacture a zoom or a nonfinite fit', () => {
  assert.deepEqual(zoomViewport(view, 2, { x: 0, y: 0 }, { width: 0, height: 0 }), view)
  const fitted = fitViewport({ x: -100, y: -50, width: 400, height: 100 }, { width: 0, height: 0 })
  assert.deepEqual(fitted, { center: { x: 100, y: 0 }, scale: 1 })
})

const atom = (id, x) => ({
  id, element: 'C', position: { x, y: 0 }, charge: 0, isotope: null, radical: 'none',
  hydrogens: { mode: 'inferred' },
})
const isolated = { id: 'pick', title: 'Pick', revision: 0, atoms: [atom('a1', 0)], bonds: [] }
const bonded = { ...isolated, atoms: [atom('a1', 0), atom('a2', 1000)],
  bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single' }] }

for (const [unitsPerPixel, insideAtom, outsideAtom, insideBond, outsideBond] of [
  [4, 35.9, 36.1, 19.9, 20.1],
  [0.25, 2.2, 2.3, 1.2, 1.3],
]) {
  test(`atom and bond hit tolerances stay 9px and 5px at ${unitsPerPixel}pt/px`, () => {
    const empty = new Set()
    const labels = new Map()
    assert.deepEqual(hitTest(isolated, { x: insideAtom, y: 0 }, empty, atomIndex(isolated), labels, unitsPerPixel), { kind: 'atom', id: 'a1' })
    assert.equal(hitTest(isolated, { x: outsideAtom, y: 0 }, empty, atomIndex(isolated), labels, unitsPerPixel), null)
    assert.deepEqual(hitTest(bonded, { x: 500, y: insideBond }, empty, atomIndex(bonded), labels, unitsPerPixel), { kind: 'bond', id: 'b1' })
    assert.equal(hitTest(bonded, { x: 500, y: outsideBond }, empty, atomIndex(bonded), labels, unitsPerPixel), null)
  })
}
