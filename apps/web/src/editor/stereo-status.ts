/**
 * The sentences that say what a gesture did to stereo, from the document before it and the reply
 * after it: the mark set or removed, the configuration stored from the drawing (and the stored
 * one it replaced), the configuration the service would not derive, and what a change of bond
 * order removed. A transaction's own `notes` list only what the neighbor cascade cleared, so a
 * record the gesture itself replaced or removed is told here and nowhere else; undo restores it.
 */
import { markFits } from '@/editor/geometry'
import type { DeriveOutcome } from '@/editor/mark-gestures'
import type { BondView, DocumentView } from '@/lib/document-contract'
import { MARK_NAMES, stereoOf, type AtomConfiguration, type BondConfiguration, type StereoMark } from '@/lib/stereo-contract'

const capital = (text: string) => `${text.charAt(0).toUpperCase()}${text.slice(1)}`
const markWords = (mark: StereoMark) => `${capital(MARK_NAMES[mark.kind])} mark`
const narrowAtom = (mark: StereoMark | null, bond: BondView) =>
  mark && 'narrow' in mark ? (mark.narrow === 'begin' ? bond.begin : bond.end) : null

const turnWords = (configuration: AtomConfiguration | BondConfiguration) => {
  switch (configuration.kind) {
    case 'tetrahedral':
      return configuration.turn
    case 'relative':
      return configuration.relation === 'same_side' ? 'same side' : 'opposite sides'
    case 'unknown':
      return 'unknown'
    default: {
      const unhandled: never = configuration
      return unhandled
    }
  }
}

/** The stored configuration of an atom or double bond in `document`, if any. */
function stored(document: DocumentView, id: string): AtomConfiguration | BondConfiguration | undefined {
  const stereo = stereoOf(document)
  return stereo.atoms.find((record) => record.atom === id)?.configuration ?? stereo.bonds.find((record) => record.bond === id)?.configuration
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

/** What storing the configuration of `item` from the drawing did, in one sentence. */
function storedFromDrawing(before: DocumentView, after: DocumentView, item: string): string | null {
  const now = stored(after, item)
  if (now === undefined) return null
  const was = stored(before, item)
  const text = `Configuration of ${item} stored from the drawing (${turnWords(now)}).`
  if (was === undefined || same(was, now)) return text
  return `${text} It replaced a stored configuration (${turnWords(was)}); undo restores it.`
}

const NEEDS: Record<StereoMark['kind'], string> = { wedge: 'single', hash: 'single', wavy: 'single', crossed: 'double' }

/**
 * The atoms or bond whose configuration the drawing of `mark` on `bond` states: the narrow end of a
 * wedge or hashed wedge, both atoms of a wavy bond, the double bond of a crossed one. Every question
 * of what the drawing still shows, or no longer shows, is answered from this one definition.
 */
const shownItems = (bond: BondView, mark: StereoMark | null) =>
  mark === null ? [] : mark.kind === 'crossed' ? [bond.id] : mark.kind === 'wavy' ? [bond.begin, bond.end] : [narrowAtom(mark, bond) ?? '']

/**
 * What a transaction did to the stereo of `before`, or null when it did nothing of the kind: a
 * mark set, changed or removed on a bond that stays, a mark that went with a deleted bond, the
 * configuration of its atoms stored from the drawing (`derive` says whether that was asked and
 * how it went), and a stored double-bond configuration removed from a bond that is no longer
 * double. A stored configuration of an atom or double bond that no remaining mark shows any more
 * is kept, and the line says so, unless this gesture stored it again.
 */
export function describeStereoChange(before: DocumentView, after: DocumentView, derive: DeriveOutcome): string | null {
  const sentences: string[] = []
  const bondsBefore = new Map(before.bonds.map((bond) => [bond.id, bond]))
  const bondsAfter = new Set(after.bonds.map((bond) => bond.id))
  // The atoms and double bonds no mark shows any more, and those this gesture stored a configuration for.
  const unshown = new Set<string>()
  const restored = new Set<string>()
  for (const bond of after.bonds) {
    const was = bondsBefore.get(bond.id)
    const wasMark = was?.mark ?? null
    if (same(wasMark, bond.mark)) {
      // A mark drawn over itself changes no mark; it can still have stored a configuration.
      if (bond.mark && derive.kind === 'sent') {
        for (const item of shownItems(bond, bond.mark)) {
          const text = same(stored(before, item), stored(after, item)) ? null : storedFromDrawing(before, after, item)
          if (text) {
            sentences.push(text)
            restored.add(item)
          }
        }
      }
      continue
    }
    if (bond.mark) {
      const where = narrowAtom(bond.mark, bond)
      sentences.push(`${markWords(bond.mark)} on ${bond.id}${where ? `, narrow end at ${where}` : ''}.`)
      const items = shownItems(bond, bond.mark)
      if (derive.kind === 'sent') {
        for (const item of items) {
          const text = storedFromDrawing(before, after, item)
          if (text) {
            sentences.push(text)
            restored.add(item)
          }
        }
      } else if (derive.kind === 'none' && derive.why) {
        sentences.push(`Nothing stored: ${derive.why}`)
      } else if (derive.kind === 'refused') {
        sentences.push(`Not stored: ${derive.message.replace(/\.$/, '')} (${derive.reason}).`)
      }
    } else if (wasMark) {
      const reason = markFits(wasMark.kind, bond.order) ? '' : `: it needs a ${NEEDS[wasMark.kind]} bond`
      sentences.push(`${markWords(wasMark)} removed from ${bond.id}${reason}.`)
    }
    if (was) {
      const stillShown = shownItems(bond, bond.mark)
      for (const item of shownItems(was, wasMark)) if (!stillShown.includes(item)) unshown.add(item)
    }
  }
  for (const bond of before.bonds) {
    if (bondsAfter.has(bond.id) || !bond.mark) continue
    // The reply's notes tell which configurations the deletion cleared; the mark itself is told here.
    sentences.push(`${markWords(bond.mark)} on ${bond.id} was removed with the bond.`)
    for (const item of shownItems(bond, bond.mark)) unshown.add(item)
  }
  // A mark drawn over itself that the service would not read a configuration for changes nothing else to say.
  if (sentences.length === 0 && derive.kind === 'refused') sentences.push(`Not stored: ${derive.message.replace(/\.$/, '')} (${derive.reason}).`)
  const shownAfter = new Set(after.bonds.flatMap((bond) => shownItems(bond, bond.mark)))
  for (const item of unshown) {
    if (stored(after, item) !== undefined && !restored.has(item) && !shownAfter.has(item)) {
      sentences.push(`The stored configuration of ${item} is kept; the drawing no longer shows it.`)
    }
  }
  for (const bond of after.bonds) {
    const lost = stored(before, bond.id)
    if (bondsBefore.has(bond.id) && lost !== undefined && stored(after, bond.id) === undefined && bond.order !== 'double') {
      sentences.push(`The stored configuration of ${bond.id} (${turnWords(lost)}) was removed: it needs a double bond. Undo restores it.`)
    }
  }
  return sentences.length > 0 ? sentences.join(' ') : null
}
