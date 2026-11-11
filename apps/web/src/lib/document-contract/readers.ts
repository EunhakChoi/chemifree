import { API_VERSION } from '@/lib/contract'
import { embeddedPngMetadata, MAX_DOCUMENT_IMAGE_PIXELS } from '@/lib/png-image'
import { isStereoNotes, isStereoView } from '@/lib/stereo-contract'
import {
  type AtomView, type BondView, CARBON_LABELS, type CaptionRun, DISPLAY_UNITS, type DocumentSettings,
  type FragmentDisplayRun, type InstanceView, LABEL_FONTS, LABEL_STYLES, LABEL_WEIGHTS, SETTING_RANGES,
  type SceneObject, type SceneReference, type SelectionView, type SettingsEdit, graphicPointCountValid,
  isSceneGraphicShape,
} from '@/lib/document-contract/model'
import {
  distinct, isAtom, isBond, isCount, isCountOrNull, isFiniteNumber, isRecord, statesAgree,
} from '@/lib/document-contract/guards'
import {
  type CopyResponse, type DocumentListResponse, type DocumentResponse, type DocumentSummary,
  type DraftFailureView, type RecoveryView, type TrashListResponse, type TrashedSummary, isStorageReason,
} from '@/lib/document-contract/responses'

export const SETTING_FIELDS: readonly (keyof DocumentSettings)[] = ['bond_length_pt', 'line_width_pt', 'label_font', 'label_weight', 'label_style', 'label_underline', 'label_outline', 'label_shadow', 'label_size_pt', 'carbon_labels', 'display_unit']

/** How a settings reader treats a field outside the schema: `closed` refuses it (the native clipboard format), `open` ignores it (readers of server replies). */
type SettingsReading = 'closed' | 'open'

/** Whether `value` is a valid value of the setting `field`; a field outside the schema is accepted only when `reading` is `open`. */
function isSettingValue(field: string, value: unknown, reading: SettingsReading): boolean {
  const range = Object.entries(SETTING_RANGES).find(([name]) => name === field)?.[1]
  if (range) return isFiniteNumber(value) && value >= range[0] && value <= range[1]
  switch (field) {
    case 'label_outline':
    case 'label_shadow':
    case 'label_underline':
      return typeof value === 'boolean'
    case 'label_style':
      return (LABEL_STYLES as readonly unknown[]).includes(value)
    case 'label_weight':
      return (LABEL_WEIGHTS as readonly unknown[]).includes(value)
    case 'label_font':
      return (LABEL_FONTS as readonly unknown[]).includes(value)
    case 'carbon_labels':
      return (CARBON_LABELS as readonly unknown[]).includes(value)
    case 'display_unit':
      return (DISPLAY_UNITS as readonly unknown[]).includes(value)
    default:
      return reading === 'open'
  }
}

/**
 * Settings fields, each valid; `complete` requires every field. A field outside the schema is
 * refused when `reading` is `closed` (a closed format) and ignored when it is `open` (readers of
 * server replies).
 */
export function isSettings(value: unknown, complete: true, reading: SettingsReading): value is DocumentSettings
export function isSettings(value: unknown, complete: false, reading: SettingsReading): value is SettingsEdit
export function isSettings(value: unknown, complete: boolean, reading: SettingsReading) {
  return (
    isRecord(value) &&
    Object.entries(value).every(([field, item]) => isSettingValue(field, item, reading)) &&
    (!complete || SETTING_FIELDS.every((field) => field in value))
  )
}

/** Complete document settings with only supported fields and bounded values. */
export function isDocumentSettings(value: unknown): value is DocumentSettings {
  return isSettings(value, true, 'closed')
}

function isRecovery(value: unknown): value is RecoveryView | null {
  return (
    value === null ||
    (isRecord(value) &&
      (value.state === 'recoverable' || value.state === 'outdated' || value.state === 'unreadable' || value.state === 'in_use') &&
      isCountOrNull(value.revision) &&
      isCountOrNull(value.base_revision) &&
      isCountOrNull(value.saved_at_ms) &&
      (value.message === null || typeof value.message === 'string'))
  )
}

/**
 * The graph and scene fields, which come together or not at all: one page, at least one
 * instance with unique IDs on that page drawing `molecule`, each with exactly one position per
 * atom, and the first instance's positions repeated as `atoms[].position`.
 */
function isScene(document: Record<string, unknown>, atoms: AtomView[]): boolean {
  const { molecule, scene } = document
  if (molecule === undefined && scene === undefined) return true
  if (!isRecord(molecule) || typeof molecule.id !== 'string' || !isRecord(scene)) return false
  const { pages, instances } = scene
  if (
    !Array.isArray(pages) ||
    pages.length !== 1 ||
    !isRecord(pages[0]) ||
    typeof pages[0].id !== 'string' ||
    !Array.isArray(instances) ||
    instances.length === 0
  ) {
    return false
  }
  const page = pages[0].id
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const instanceIds = new Set<string>()
  for (const instance of instances) {
    if (
      !isRecord(instance) ||
      typeof instance.id !== 'string' ||
      instanceIds.has(instance.id) ||
      instance.page !== page ||
      instance.molecule !== molecule.id ||
      !Array.isArray(instance.positions)
    ) {
      return false
    }
    instanceIds.add(instance.id)
    const placed = new Set<string>()
    for (const position of instance.positions) {
      if (
        !isRecord(position) ||
        typeof position.atom !== 'string' ||
        !atomIds.has(position.atom) ||
        placed.has(position.atom) ||
        !isFiniteNumber(position.x) ||
        !isFiniteNumber(position.y)
      ) {
        return false
      }
      placed.add(position.atom)
    }
    if (placed.size !== atomIds.size) return false
  }
  // Every instance, the first included, was checked above.
  const first = new Map((instances[0] as InstanceView).positions.map((position) => [position.atom, position]))
  return atoms.every((atom) => {
    const position = first.get(atom.id)
    return position !== undefined && position.x === atom.position.x && position.y === atom.position.y
  })
}

export function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string') && distinct(value)
}

export function isSelection(value: unknown): value is SelectionView {
  return isRecord(value) && isStringArray(value.objects) && isStringArray(value.atoms) && isStringArray(value.bonds)
}

/** Canonical opaque sRGB override; the length check also excludes final line terminators. */
export function isCaptionColor(value: unknown): value is string {
  return typeof value === 'string' && value.length === 7 && /^#[0-9A-F]{6}$/.test(value)
}

/** Strict Unicode scalar text, preserving run order and bytes without normalization. */
export function validCaptionRuns(value: unknown): value is CaptionRun[] {
  if (!Array.isArray(value) || value.length === 0 || value.length > 32) return false
  let bytes = 0
  let visible = false
  for (const run of value) {
    if (!isRecord(run) || typeof run.text !== 'string' || run.text.length === 0 ||
        (typeof run.script !== 'string' || !['baseline', 'subscript', 'superscript'].includes(run.script)) ||
        (run.weight !== 'normal' && run.weight !== 'bold') ||
        (run.style !== 'normal' && run.style !== 'italic') ||
        (run.shadow !== undefined && typeof run.shadow !== 'boolean') ||
        (run.outline !== undefined && typeof run.outline !== 'boolean') ||
        (run.underline !== undefined && typeof run.underline !== 'boolean') ||
        (run.color !== undefined && run.color !== null && !isCaptionColor(run.color)) ||
        (run.size_pt !== undefined && run.size_pt !== null && (!isFiniteNumber(run.size_pt) || run.size_pt < 4 || run.size_pt > 72)) ||
        (run.font !== undefined && run.font !== null && !LABEL_FONTS.some((font) => font === run.font)) ||
        /[\p{Cc}\u2028\u2029]/u.test(run.text)) return false
    // JSON can carry unpaired UTF-16 surrogates, which Rust String deliberately rejects.
    for (const character of run.text) {
      const code = character.codePointAt(0)
      if (code !== undefined && code >= 0xd800 && code <= 0xdfff) return false
    }
    bytes += new TextEncoder().encode(run.text).length
    if (/[^\p{White_Space}]/u.test(run.text)) visible = true
  }
  return bytes <= 16_384 && visible
}

function isSceneObject(value: unknown): value is SceneObject {
  if (!isRecord(value) || typeof value.id !== 'string' || (value.parent !== null && typeof value.parent !== 'string')) return false
  switch (value.kind) {
    case 'instance':
      return typeof value.molecule === 'string'
    case 'group':
      return isStringArray(value.children)
    case 'image': {
      const metadata = value.mime_type === 'image/png' ? embeddedPngMetadata(value.data_base64) : null
      return metadata !== null && value.pixel_width === metadata.pixelWidth && value.pixel_height === metadata.pixelHeight &&
        isRecord(value.origin) && isFiniteNumber(value.origin.x) && isFiniteNumber(value.origin.y) &&
        isRecord(value.size_pt) && isFiniteNumber(value.size_pt.x) && isFiniteNumber(value.size_pt.y) && value.size_pt.x > 0 && value.size_pt.y > 0 &&
        Math.abs(value.origin.x) <= 100_000 && Math.abs(value.origin.y) <= 100_000 &&
        Math.abs(value.origin.x + value.size_pt.x) <= 100_000 && Math.abs(value.origin.y + value.size_pt.y) <= 100_000
    }
    case 'caption':
      return (value.alignment === 'left' || value.alignment === 'center' || value.alignment === 'right') && isRecord(value.anchor) && isFiniteNumber(value.anchor.x) && isFiniteNumber(value.anchor.y) && validCaptionRuns(value.runs)
    case 'graphic':
      return (
        isSceneGraphicShape(value.shape) &&
        (value.line_width_pt === undefined || value.line_width_pt === null || (isFiniteNumber(value.line_width_pt) && value.line_width_pt >= SETTING_RANGES.line_width_pt[0] && value.line_width_pt <= SETTING_RANGES.line_width_pt[1])) &&
        Array.isArray(value.points) && graphicPointCountValid(value.shape, value.points.length) &&
        value.points.every((point) => isRecord(point) && isFiniteNumber(point.x) && isFiniteNumber(point.y) && ((value.shape !== 'polygon' && value.shape !== 'spline') || (Math.abs(point.x) <= 100_000 && Math.abs(point.y) <= 100_000)))
      )
    default:
      return false
  }
}

function isSceneReference(value: unknown): value is SceneReference {
  return isRecord(value) && typeof value.source === 'string' && isRecord(value.target) &&
    (value.target.kind === 'object' || value.target.kind === 'atom' || value.target.kind === 'bond') &&
    typeof value.target.id === 'string'
}

/** The optional scene tree and selection must arrive together and describe one ordered tree. */
function isSceneTree(document: Record<string, unknown>, atoms: AtomView[], bonds: BondView[]): boolean {
  const { scene, selection } = document
  if (scene === undefined) return selection === undefined
  if (!isRecord(scene)) return false
  if (scene.roots === undefined && scene.objects === undefined && scene.references === undefined && selection === undefined) return true
  if (!isStringArray(scene.roots) || !Array.isArray(scene.objects) || !scene.objects.every(isSceneObject) ||
      !Array.isArray(scene.references) || !scene.references.every(isSceneReference) ||
      !Array.isArray(scene.instances) || !isSelection(selection) || !isRecord(document.molecule)) return false
  const sceneObjects = scene.objects
  if (sceneObjects.reduce((total, object) => total + (object.kind === 'image' ? object.pixel_width * object.pixel_height : 0), 0) > MAX_DOCUMENT_IMAGE_PIXELS) return false
  const objects = new Map(sceneObjects.map((object) => [object.id, object]))
  if (objects.size !== sceneObjects.length) return false
  const visited = new Set<string>()
  const order: string[] = []
  const stack = scene.roots.slice().reverse().map((id) => ({ id, parent: null as string | null, depth: 1 }))
  while (stack.length) {
    const current = stack.pop()
    if (!current) return false
    const object = objects.get(current.id)
    if (!object || visited.has(current.id) || object.parent !== current.parent || current.depth > 16) return false
    visited.add(current.id)
    order.push(current.id)
    if (object.kind === 'instance' && object.molecule !== document.molecule.id) return false
    if (object.kind === 'group') {
      for (const child of object.children.slice().reverse()) stack.push({ id: child, parent: object.id, depth: current.depth + 1 })
    }
  }
  if (visited.size !== objects.size || order.some((id, index) => sceneObjects[index].id !== id)) return false
  const instanceIds = new Set(scene.instances.map((instance) => isRecord(instance) ? instance.id : undefined))
  const objectInstances = sceneObjects.filter((object) => object.kind === 'instance')
  if (objectInstances.length !== instanceIds.size || objectInstances.some((object) => !instanceIds.has(object.id))) return false
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const bondIds = new Set(bonds.map((bond) => bond.id))
  const links = new Set<string>()
  for (const reference of scene.references) {
    const key = `${reference.source}:${reference.target.kind}:${reference.target.id}`
    const targets = reference.target.kind === 'object' ? objects : reference.target.kind === 'atom' ? atomIds : bondIds
    if (!objects.has(reference.source) || !targets.has(reference.target.id) || links.has(key)) return false
    links.add(key)
  }
  return selection.objects.every((id) => objects.has(id)) &&
    selection.atoms.every((id) => atomIds.has(id)) && selection.bonds.every((id) => bondIds.has(id))
}

/** Mirrors the shared caption text profile; payload length is UTF-8 bytes. */
export function validDisplayLabel(value: unknown): value is string {
  if (typeof value !== 'string' || new TextEncoder().encode(value).length > 128 ||
    !/[^\p{White_Space}]/u.test(value)) return false
  // Iterate code points so valid supplementary Unicode stays accepted while lone
  // surrogates and the caption profile's C0/C1 controls are refused explicitly.
  return [...value].every((character) => {
    const point = character.codePointAt(0)
    return point !== undefined && point >= 0x20 && (point < 0x7f || point > 0x9f) &&
      point !== 0x2028 && point !== 0x2029 && (point < 0xd800 || point > 0xdfff)
  })
}

/** Reject stale or overlapping compact metadata rather than hiding real graph content. */
function isAbbreviations(document: Record<string, unknown>, atoms: AtomView[], bonds: BondView[]): boolean {
  const value = document.abbreviations
  if (value === undefined) return true
  if (!Array.isArray(value)) return false
  if (value.length === 0) return true
  if (!isRecord(document.scene) || !Array.isArray(document.scene.instances) || document.scene.instances.length !== 1) return false
  const placement = document.scene.instances[0]
  if (!isRecord(placement)) return false
  const members = new Set<string>()
  for (const item of value) {
    if (!isRecord(item) || item.instance !== placement.id || typeof item.anchor !== 'string' || typeof item.terminal !== 'string' || !validDisplayLabel(item.display_label) || item.anchor === item.terminal) return false
    const pair = [item.anchor, item.terminal]
    if (pair.some((id) => members.has(id))) return false
    for (const id of pair) {
      const atom = atoms.find((atom) => atom.id === id)
      if (!atom || atom.kind !== 'element' || atom.element !== 'C' || atom.charge !== 0 || atom.isotope !== null || atom.hydrogens?.mode !== 'inferred' || atom.radical !== 'none' || atom.attachment_points?.length !== 0) return false
      members.add(id)
    }
    let internal = 0
    let boundary = 0
    const local = new Set(pair)
    for (const bond of bonds) {
      const begin = pair.includes(bond.begin)
      const end = pair.includes(bond.end)
      if (!begin && !end) continue
      if (bond.order !== 'single' || bond.mark != null) return false
      if (begin && end) internal += 1
      else {
        if ((begin && bond.begin !== item.anchor) || (end && bond.end !== item.anchor)) return false
        boundary += 1
        local.add(begin ? bond.end : bond.begin)
      }
    }
    if (internal !== 1 || boundary !== 1) return false
    if (bonds.some((bond) => bond.mark != null && (local.has(bond.begin) || local.has(bond.end)))) return false
    if (isRecord(document.stereo)) {
      const stereo = document.stereo
      if (Array.isArray(stereo.atoms) && stereo.atoms.some((record) => isRecord(record) && typeof record.atom === 'string' && local.has(record.atom))) return false
      if (Array.isArray(stereo.bonds) && stereo.bonds.some((record) => isRecord(record) && bonds.some((bond) => bond.id === record.bond && (local.has(bond.begin) || local.has(bond.end))))) return false
      if (Array.isArray(stereo.groups) && stereo.groups.some((group) => isRecord(group) && Array.isArray(group.members) && group.members.some((id) => typeof id === 'string' && local.has(id)))) return false
    }
  }
  return true
}

/** Nonempty scalar runs share the display label byte and single-line profile. */
export function validFragmentDisplayRuns(value: unknown): value is FragmentDisplayRun[] {
  if (!Array.isArray(value) || value.length === 0) return false
  const texts: string[] = []
  for (const item of value) {
    if (!isRecord(item) || typeof item.text !== 'string' || item.text.length === 0 ||
      (item.script !== 'baseline' && item.script !== 'subscript' && item.script !== 'superscript') ||
      /[\p{Cc}\u2028\u2029\uD800-\uDFFF]/u.test(item.text)) return false
    texts.push(item.text)
  }
  return validDisplayLabel(texts.join(''))
}

/** Validate retained fragment metadata before any rendering can hide atoms. */
function isFragmentDisplays(document: Record<string, unknown>, atoms: AtomView[], bonds: BondView[]): boolean {
  const value = document.fragment_displays
  // Older snapshots predate fragment display metadata.
  if (value === undefined) return true
  if (!Array.isArray(value)) return false
  if (value.length === 0) return true
  if (!isRecord(document.scene) || !Array.isArray(document.scene.instances) || document.scene.instances.length !== 1) return false
  const placement = document.scene.instances[0]
  if (!isRecord(placement)) return false
  const occupied = new Set<string>()
  if (Array.isArray(document.abbreviations)) for (const item of document.abbreviations) {
    if (isRecord(item) && typeof item.anchor === 'string' && typeof item.terminal === 'string') {
      occupied.add(item.anchor); occupied.add(item.terminal)
    }
  }
  for (const item of value) {
    if (!isRecord(item) || item.instance !== placement.id || typeof item.anchor !== 'string' || !validDisplayLabel(item.text) ||
      (item.runs !== undefined && (!validFragmentDisplayRuns(item.runs) || item.runs.map((run) => run.text).join('') !== item.text)) ||
      !Array.isArray(item.members) || item.members.length === 0 || !item.members.every((id): id is string => typeof id === 'string')) return false
    const members = new Set(item.members)
    if (members.size !== item.members.length || !members.has(item.anchor)) return false
    for (const id of members) {
      const atom = atoms.find((entry) => entry.id === id)
      if (occupied.has(id) || !atom || atom.kind !== 'element' || atom.charge !== 0 || atom.isotope !== null || atom.hydrogens?.mode !== 'inferred' || atom.radical !== 'none' || atom.attachment_points?.length !== 0) return false
      occupied.add(id)
    }
    const local = new Set(members)
    const internal = bonds.filter((bond) => members.has(bond.begin) && members.has(bond.end))
    const boundary = bonds.filter((bond) => members.has(bond.begin) !== members.has(bond.end))
    if (internal.length !== members.size - 1 || boundary.length !== 1 || internal.some((bond) => bond.order !== 'single' || bond.mark != null)) return false
    const edge = boundary[0]
    if (edge.order !== 'single' || edge.mark != null || (members.has(edge.begin) ? edge.begin : edge.end) !== item.anchor) return false
    local.add(members.has(edge.begin) ? edge.end : edge.begin)
    const visited = new Set([item.anchor])
    let previous = -1
    while (visited.size !== previous) {
      previous = visited.size
      for (const bond of internal) if (visited.has(bond.begin) || visited.has(bond.end)) { visited.add(bond.begin); visited.add(bond.end) }
    }
    if (visited.size !== members.size || bonds.some((bond) => bond.mark != null && (local.has(bond.begin) || local.has(bond.end)))) return false
    if (isRecord(document.stereo)) {
      const stereo = document.stereo
      if (Array.isArray(stereo.atoms) && stereo.atoms.some((record) => isRecord(record) && typeof record.atom === 'string' && local.has(record.atom))) return false
      if (Array.isArray(stereo.bonds) && stereo.bonds.some((record) => isRecord(record) && bonds.some((bond) => bond.id === record.bond && (local.has(bond.begin) || local.has(bond.end))))) return false
      if (Array.isArray(stereo.groups) && stereo.groups.some((group) => isRecord(group) && Array.isArray(group.members) && group.members.some((id) => typeof id === 'string' && local.has(id)))) return false
    }
  }
  return true
}

/** Rejects malformed snapshots, including bonds that name atoms the snapshot lacks. */
export function isDocumentResponse(value: unknown): value is DocumentResponse {
  if (!isRecord(value) || value.api_version !== API_VERSION) return false
  const { document, session } = value
  if (
    !isRecord(document) ||
    typeof document.id !== 'string' ||
    typeof document.title !== 'string' ||
    !isCount(document.revision) ||
    !Array.isArray(document.atoms) ||
    !Array.isArray(document.bonds) ||
    !document.atoms.every(isAtom) ||
    !document.bonds.every(isBond)
  ) {
    return false
  }
  const atomIds = new Set(document.atoms.map((atom) => atom.id))
  const bondIds = new Set(document.bonds.map((bond) => bond.id))
  return (
    atomIds.size === document.atoms.length &&
    bondIds.size === document.bonds.length &&
    document.bonds.every((bond) => atomIds.has(bond.begin) && atomIds.has(bond.end)) &&
    statesAgree(document.atoms, document.bonds) &&
    (document.stereo === undefined || isStereoView(document.stereo, document.atoms, document.bonds)) &&
    isScene(document, document.atoms) &&
    isAbbreviations(document, document.atoms, document.bonds) &&
    isFragmentDisplays(document, document.atoms, document.bonds) &&
    isSceneTree(document, document.atoms, document.bonds) &&
    (document.settings === undefined || isSettings(document.settings, true, 'open')) &&
    isRecord(session) &&
    isCount(session.saved_revision) &&
    typeof session.unsaved_changes === 'boolean' &&
    isCount(session.undo_depth) &&
    isCount(session.redo_depth) &&
    isCountOrNull(session.draft_revision) &&
    isDraftFailure(session.draft_failure) &&
    isRecovery(value.recovery) &&
    (value.notes === undefined || isStereoNotes(value.notes))
  )
}

function isDraftFailure(value: unknown): value is DraftFailureView | null | undefined {
  return (
    value == null ||
    (isRecord(value) &&
      isCount(value.attempts) &&
      isCount(value.retry_in_ms) &&
      (value.code === undefined || typeof value.code === 'string') &&
      (value.reason === undefined || isStorageReason(value.reason)) &&
      typeof value.message === 'string')
  )
}

export function isCopy(value: unknown): value is CopyResponse {
  if (!isDocumentResponse(value)) return false
  const { source } = value as { source?: unknown }
  return (
    isRecord(source) &&
    typeof source.id === 'string' &&
    typeof source.closed === 'boolean' &&
    typeof source.discarded_changes === 'boolean'
  )
}

function isSummary(value: unknown): value is DocumentSummary {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    isCount(value.revision) &&
    isCount(value.atom_count) &&
    isCount(value.bond_count) &&
    typeof value.open === 'boolean' &&
    typeof value.unsaved_changes === 'boolean' &&
    isRecovery(value.recovery) &&
    isCountOrNull(value.saved_at_ms)
  )
}

function isTrashed(value: unknown): value is TrashedSummary {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    isCount(value.revision) &&
    isCount(value.atom_count) &&
    isCount(value.bond_count) &&
    isCountOrNull(value.deleted_at_ms)
  )
}

export function isTrashList(value: unknown): value is TrashListResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    Array.isArray(value.documents) &&
    value.documents.every(isTrashed) &&
    Array.isArray(value.unreadable) &&
    value.unreadable.every((item) => isRecord(item) && typeof item.id === 'string' && typeof item.message === 'string') &&
    (value.truncated === undefined || typeof value.truncated === 'boolean')
  )
}

/** A delete or restore reply: which document moved, and its title. */
export function isMoved(value: unknown): value is { id: string; title: string } {
  return isRecord(value) && value.api_version === API_VERSION && typeof value.id === 'string' && typeof value.title === 'string'
}

export function isList(value: unknown): value is DocumentListResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    Array.isArray(value.documents) &&
    value.documents.every(isSummary) &&
    Array.isArray(value.unreadable) &&
    value.unreadable.every((item) => isRecord(item) && typeof item.id === 'string' && typeof item.message === 'string') &&
    (value.truncated === undefined || typeof value.truncated === 'boolean')
  )
}

export function isClose(value: unknown): value is { discarded_changes: boolean } {
  return isRecord(value) && value.api_version === API_VERSION && typeof value.discarded_changes === 'boolean'
}
