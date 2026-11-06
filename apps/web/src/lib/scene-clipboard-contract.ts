import type { DocumentView, NativeClipboardIdMapping, Position, SceneObject, SceneReference, SelectionView } from '@/lib/document-contract'

type GraphicObject = Extract<SceneObject, { kind: 'graphic' | 'group' }>
type PayloadObject = Omit<Extract<GraphicObject, { kind: 'graphic' }>, 'parent'> | Omit<Extract<GraphicObject, { kind: 'group' }>, 'parent'>

interface GraphicPayload {
  settings: Record<string, unknown>
  roots: string[]
  objects: PayloadObject[]
  references: SceneReference[]
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function exactKeys(value: Record<string, unknown>, keys: string[]): boolean {
  return Object.keys(value).length === keys.length && keys.every((key) => key in value)
}

function objectId(value: unknown): value is string {
  return typeof value === 'string' && /^s[1-9][0-9]*$/.test(value) && Number(value.slice(1)) <= 0xffff_ffff
}

function ids(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((id) => objectId(id)) && new Set(value).size === value.length
}

function point(value: unknown): value is Position {
  return record(value) && exactKeys(value, ['x', 'y']) && typeof value.x === 'number' && typeof value.y === 'number' &&
    Number.isFinite(value.x) && Number.isFinite(value.y) && Math.abs(value.x) <= 100_000 && Math.abs(value.y) <= 100_000
}

function equal(left: unknown, right: unknown, exactNumbers = false): boolean {
  if (exactNumbers && typeof left === 'number' && typeof right === 'number') return Object.is(left, right)
  if (left === right) return true
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left) && Array.isArray(right) && left.length === right.length && left.every((item, index) => equal(item, right[index], exactNumbers))
  }
  return record(left) && record(right) && Object.keys(left).length === Object.keys(right).length &&
    Object.keys(left).every((key) => key in right && equal(left[key], right[key], exactNumbers))
}

function referenceKey(reference: SceneReference): string {
  return `${reference.source}:${reference.target.kind}:${reference.target.id}`
}

function sameReferences(first: SceneReference[], second: SceneReference[]): boolean {
  const keys = new Set(first.map((reference) => referenceKey(reference)))
  return keys.size === first.length && first.length === second.length &&
    new Set(second.map((reference) => referenceKey(reference))).size === second.length &&
    second.every((reference) => keys.has(referenceKey(reference)))
}

/** Only used after a successful service reply; invalid input is still diagnosed by the service. */
function payloadOf(text: string): GraphicPayload | null {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    return null
  }
  if (!record(value) || !exactKeys(value, ['format', 'version', 'kind', 'settings', 'roots', 'objects', 'references']) ||
      value.format !== 'chemifree.clipboard' || value.version !== 2 || value.kind !== 'graphics' || !record(value.settings) ||
      !ids(value.roots) || value.roots.length === 0 || !Array.isArray(value.objects) || value.objects.length > 1000 ||
      !Array.isArray(value.references) || value.references.length > 4096 || value.objects.length + value.references.length > 500) return null
  const objects: PayloadObject[] = []
  for (const item of value.objects) {
    if (!record(item) || !objectId(item.id) || objects.some((prior) => prior.id === item.id)) return null
    if (item.kind === 'group' && exactKeys(item, ['kind', 'id', 'children']) && ids(item.children) && item.children.length >= 2) {
      objects.push({ kind: 'group', id: item.id, children: item.children })
    } else if (item.kind === 'graphic' && exactKeys(item, ['kind', 'id', 'shape', 'points']) &&
        (item.shape === 'line' || item.shape === 'arrow' || item.shape === 'rectangle') &&
        Array.isArray(item.points) && item.points.length === 2 && point(item.points[0]) && point(item.points[1])) {
      objects.push({ kind: 'graphic', id: item.id, shape: item.shape, points: [item.points[0], item.points[1]] })
    } else return null
  }
  const byId = new Map(objects.map((object) => [object.id, object]))
  const seen = new Set<string>()
  const visit = (id: string, depth: number): boolean => {
    const object = byId.get(id)
    if (!object || seen.has(id) || depth > 16) return false
    seen.add(id)
    return object.kind !== 'group' || object.children.every((child) => visit(child, depth + 1))
  }
  if (!value.roots.every((root) => visit(root, 1)) || seen.size !== objects.length) return null
  // Wire object records may be in any order; containment alone determines paint order.
  const order = [...seen]
  objects.sort((first, second) => order.indexOf(first.id) - order.indexOf(second.id))
  const references: SceneReference[] = []
  for (const item of value.references) {
    if (!record(item) || !exactKeys(item, ['source', 'target']) || !objectId(item.source) || !seen.has(item.source) ||
        !record(item.target) || !exactKeys(item.target, ['kind', 'id']) || item.target.kind !== 'object' ||
        !objectId(item.target.id) || !seen.has(item.target.id)) return null
    references.push({ source: item.source, target: { kind: 'object', id: item.target.id } })
  }
  if (!sameReferences(references, references)) return null
  return { settings: { label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, ...value.settings }, roots: value.roots, objects, references }
}

/** Checks the entire copied forest against the requested source scope without following references. */
export function sceneClipboardCopyMatches(
  text: string, copied: SelectionView, roots: string[], requested: SelectionView, source: DocumentView, settings: unknown,
): boolean {
  const payload = payloadOf(text)
  const scene = source.scene
  if (!payload || !scene?.objects || !scene.roots || requested.atoms.length || requested.bonds.length ||
      requested.objects.length === 0 || copied.atoms.length || copied.bonds.length || !equal(payload.settings, settings)) return false
  const byId = new Map(scene.objects.map((object) => [object.id, object]))
  const wanted = new Set(requested.objects)
  if (wanted.size !== requested.objects.length || requested.objects.some((id) => !byId.has(id))) return false
  const expectedRoots: string[] = []
  const objects: PayloadObject[] = []
  const visit = (id: string, included: boolean): boolean => {
    const object = byId.get(id)
    if (!object) return false
    const selected = included || wanted.has(id)
    if (!included && selected) expectedRoots.push(id)
    if (selected) {
      if (object.kind === 'instance' || object.kind === 'caption') return false
      if (object.kind === 'group') objects.push({ kind: object.kind, id, children: object.children })
      else objects.push({ kind: object.kind, id, shape: object.shape, points: object.points })
    }
    return object.kind !== 'group' || object.children.every((child) => visit(child, selected))
  }
  if (!scene.roots.every((root) => visit(root, false))) return false
  const included = new Set(objects.map((object) => object.id))
  const references = (scene.references ?? []).filter((reference) => included.has(reference.source))
  return equal(roots, expectedRoots) && equal(payload.roots, expectedRoots) &&
    equal(copied.objects, objects.map((object) => object.id)) && equal(payload.objects, objects) && sameReferences(payload.references, references)
}

/** Incoming links belong to surviving objects and must never be discarded by Cut. */
export function sceneClipboardCutHasIncoming(source: DocumentView, copied: string[]): boolean {
  const removed = new Set(copied)
  return (source.scene?.references ?? []).some((reference) => !removed.has(reference.source) &&
    reference.target.kind === 'object' && removed.has(reference.target.id))
}

/** Cut removes exactly the copied forest and its owned links, retaining outside parents. */
export function sceneClipboardCutMatches(before: DocumentView, after: DocumentView, copied: string[]): boolean {
  const source = before.scene
  const target = after.scene
  const removed = new Set(copied)
  if (!source?.objects || !source.roots || !target || removed.size === 0 ||
      sceneClipboardCutHasIncoming(before, copied) || !equal(after.selection, { objects: [], atoms: [], bonds: [] })) return false
  const objects = source.objects.filter((object) => !removed.has(object.id)).map((object) => object.kind === 'group'
    ? { ...object, children: object.children.filter((child) => !removed.has(child)) } : object)
  if (!equal(target, { ...source, roots: source.roots.filter((id) => !removed.has(id)), objects,
    references: (source.references ?? []).filter((reference) => !removed.has(reference.source)) }, true)) return false
  for (const key of ['id', 'title', 'atoms', 'bonds', 'molecule', 'settings', 'stereo'] as const) {
    if (!equal(before[key], after[key], true)) return false
  }
  return true
}

/** Verifies fresh mappings, the translated forest, its append order and every unchanged destination entity. */
export function sceneClipboardPasteMapping(
  text: string, mapping: unknown, offset: Position, before: DocumentView, after: DocumentView, settings: unknown,
): NativeClipboardIdMapping[] | null {
  const payload = payloadOf(text)
  const sourceScene = before.scene
  const targetScene = after.scene
  const selection = after.selection
  if (!payload || !sourceScene?.objects || !sourceScene.roots || !targetScene?.objects || !targetScene.roots || !selection ||
      selection.atoms.length || selection.bonds.length || !equal(payload.settings, settings) || !record(mapping) ||
      !exactKeys(mapping, ['objects']) || !Array.isArray(mapping.objects) || mapping.objects.length !== payload.objects.length) return null
  const mapped: NativeClipboardIdMapping[] = []
  const previous = new Map(sourceScene.objects.map((object) => [object.id, object]))
  for (const [index, item] of mapping.objects.entries()) {
    if (!record(item) || !exactKeys(item, ['source', 'target']) || item.source !== payload.objects[index]?.id ||
        !objectId(item.source) || !objectId(item.target) || previous.has(item.target) || mapped.some((prior) => prior.target === item.target)) return null
    mapped.push({ source: item.source, target: item.target })
  }
  const targets = new Map(mapped.map((item) => [item.source, item.target]))
  const expectedRoots = payload.roots.map((root) => targets.get(root))
  if (!equal(targetScene.roots, [...sourceScene.roots, ...expectedRoots]) || selection.objects.length !== expectedRoots.length ||
      new Set(selection.objects).size !== selection.objects.length || !selection.objects.every((id) => expectedRoots.includes(id)) ||
      targetScene.objects.length !== previous.size + mapped.length) return null
  const actual = new Map(targetScene.objects.map((object) => [object.id, object]))
  if (sourceScene.objects.some((object) => !equal(actual.get(object.id), object))) return null
  const parent = new Map<string, string>()
  for (const object of payload.objects) {
    if (object.kind === 'group') {
      for (const child of object.children) parent.set(child, object.id)
    }
  }
  for (const object of payload.objects) {
    const expected = object.kind === 'group'
      ? { ...object, id: targets.get(object.id), parent: targets.get(parent.get(object.id) ?? '') ?? null,
        children: object.children.map((child) => targets.get(child)) }
      : { ...object, id: targets.get(object.id), parent: targets.get(parent.get(object.id) ?? '') ?? null,
        points: object.points.map((point) => ({ x: point.x + offset.x, y: point.y + offset.y })) }
    const target = targets.get(object.id)
    if (!target || !equal(actual.get(target), expected)) return null
  }
  const references: SceneReference[] = []
  for (const reference of payload.references) {
    const source = targets.get(reference.source)
    const target = targets.get(reference.target.id)
    if (!source || !target) return null
    references.push({ source, target: { kind: 'object', id: target } })
  }
  if (!sameReferences(targetScene.references ?? [], [...(sourceScene.references ?? []), ...references])) return null
  // Revision and root selection are the only non-scene changes permitted by this profile.
  for (const key of ['id', 'title', 'atoms', 'bonds', 'molecule', 'settings', 'stereo'] as const) {
    if (!equal(before[key], after[key])) return null
  }
  if (!equal(sourceScene.pages, targetScene.pages) || !equal(sourceScene.instances, targetScene.instances)) return null
  return mapped
}
