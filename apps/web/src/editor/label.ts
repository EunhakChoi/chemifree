/**
 * The layout of an atom's label: its text (element or query label, stated or unknown
 * hydrogens, charge, isotope), its state marks (radical dots, attachment points), where each
 * goes relative to the atom, and one box that encloses all of them. Bond trimming, the canvas
 * view box, and hit testing all use that box, so nothing is drawn outside the space they
 * reserve. Every width is fixed here and given to the SVG as `textLength`, so the drawn text
 * takes exactly the computed width whatever font renders it.
 *
 * Nothing here computes chemistry: inferred hydrogens are not counted and add nothing.
 */
import type { AtomView, QueryAtom } from '@/lib/document-contract'

/** A rectangle in document points; `x`, `y` is its top-left corner. */
export interface Box {
  x: number
  y: number
  width: number
  height: number
}

/** One run of label text, placed relative to the atom: `dx` is its left edge, `dy` its middle. */
export interface LabelText {
  text: string
  dx: number
  dy: number
  size: number
  /** The width the text is drawn at. */
  width: number
  part: 'isotope' | 'symbol' | 'hydrogens' | 'hydrogen-count' | 'charge'
}

/** A dot of a radical mark, relative to the atom; a hollow one is drawn as a ring. */
export interface LabelDot {
  dx: number
  dy: number
  hollow: boolean
}

export interface AtomLabel {
  /** The label's text runs, left to right, drawn as one text element. */
  texts: LabelText[]
  /** Small text marks: the attachment points, and a radical stated as unknown. */
  marks: Array<LabelText & { mark: 'attachment' | 'radical' }>
  dots: LabelDot[]
  /** Encloses every text and mark, relative to the atom. */
  box: Box
  /** The full description, with the full query expression, for the accessible name. */
  name: string
}

export const LABEL_SIZE = 12
const SMALL_SIZE = 8
const MARK_SIZE = 7
/** Half the height a run of text may take, as a share of its font size, measured generously. */
const HALF_HEIGHT = 0.7
const DOT_RADIUS = 1.4
const PADDING = 1
/** List queries name at most this many elements on the canvas. */
export const SHOWN_QUERY_ELEMENTS = 3

/** Advance of one character as a share of the font size; drawn text is fitted to it. */
function advance(character: string) {
  if (/[A-Z]/.test(character)) return 0.72
  if (/[a-z]/.test(character)) return 0.56
  if (/[0-9]/.test(character)) return 0.6
  if (character === '…') return 0.9
  if (character === '+' || character === '−' || character === '?') return 0.62
  return 0.4
}

export function textWidth(text: string, size: number) {
  return [...text].reduce((total, character) => total + advance(character) * size, 0)
}

export function chargeText(charge: number) {
  if (charge === 0) return ''
  const sign = charge > 0 ? '+' : '−'
  return Math.abs(charge) === 1 ? sign : `${Math.abs(charge)}${sign}`
}

const NAMED_QUERY: Record<Exclude<QueryAtom['kind'], 'list'>, { label: string; name: string }> = {
  any: { label: '*', name: 'any atom' },
  any_heavy: { label: 'A', name: 'any atom except hydrogen' },
  heteroatom: { label: 'Q', name: 'any atom except carbon and hydrogen' },
  halogen: { label: 'X', name: 'any halogen' },
}

/** A query's canvas label: a list shows at most three elements, then "…". */
export function queryLabel(query: QueryAtom) {
  if (query.kind !== 'list') return NAMED_QUERY[query.kind].label
  const shown = query.elements.slice(0, SHOWN_QUERY_ELEMENTS)
  const more = query.elements.length > SHOWN_QUERY_ELEMENTS ? ',…' : ''
  return `${query.negated ? '!' : ''}[${shown.join(',')}${more}]`
}

/** A query's full expression in words. */
export function queryDescription(query: QueryAtom) {
  if (query.kind !== 'list') return NAMED_QUERY[query.kind].name
  return `${query.negated ? 'any atom except' : 'one of'} ${query.elements.join(', ')}`
}

/** The atom's recorded states in words, for its accessible name and the inspector. */
export function statesDescription(atom: AtomView) {
  const parts: string[] = []
  if (atom.hydrogens.mode === 'specified') parts.push(`${atom.hydrogens.count} stated hydrogens`)
  if (atom.hydrogens.mode === 'unknown') parts.push('hydrogens unknown')
  if (atom.radical !== 'none') parts.push(`${atom.radical} radical`)
  if (atom.attachment_points.length > 0) parts.push(`attachment points ${atom.attachment_points.join(', ')}`)
  if (atom.charge !== 0) parts.push(`charge ${atom.charge > 0 ? '+' : ''}${atom.charge}`)
  if (atom.isotope !== null) parts.push(`isotope ${atom.isotope}`)
  return parts
}

/**
 * A carbon is drawn as a bare vertex only while it is bonded and carries nothing recorded:
 * a charge, isotope, stated or unknown hydrogens, a radical, or attachment points label it.
 * Inferred hydrogens do not. Every other element and every query atom is labelled.
 */
export function showsLabel(atom: AtomView, degree: number) {
  return (
    atom.kind === 'query' ||
    atom.element !== 'C' ||
    degree === 0 ||
    statesDescription(atom).length > 0
  )
}

function boxOf(item: { dx: number; dy: number; width: number; size: number }): Box {
  const half = item.size * HALF_HEIGHT
  return { x: item.dx, y: item.dy - half, width: item.width, height: 2 * half }
}

/** The smallest box enclosing all of `boxes`. */
export function union(boxes: Box[]): Box {
  const left = Math.min(...boxes.map((box) => box.x))
  const top = Math.min(...boxes.map((box) => box.y))
  const right = Math.max(...boxes.map((box) => box.x + box.width))
  const bottom = Math.max(...boxes.map((box) => box.y + box.height))
  return { x: left, y: top, width: right - left, height: bottom - top }
}

/**
 * The complete label of `atom` with `degree` bonds, or null for a bare carbon vertex. The
 * symbol is centred on the atom. Stated or unknown hydrogens go beside it with their count as
 * a subscript ("?" when unknown): after it, or before it ("H3C") when `hydrogensLeft` because
 * the atom's bonds lean right. The isotope is a superscript before the symbol and the charge
 * one after it. Radical dots sit above the symbol and attachment points below it.
 */
export function atomLabel(atom: AtomView, degree: number, hydrogensLeft = false): AtomLabel | null {
  if (!showsLabel(atom, degree)) return null
  const run = (text: string, size: number, dx: number, dy: number, part: LabelText['part']): LabelText => ({
    text,
    dx,
    dy,
    size,
    width: textWidth(text, size),
    part,
  })
  const symbol = atom.kind === 'query' && atom.query ? queryLabel(atom.query) : atom.element
  const symbolWidth = textWidth(symbol, LABEL_SIZE)
  // Runs left of the symbol are listed nearest first and placed leftward; runs right of it,
  // rightward. `texts` keeps reading order.
  const left: Array<[string, number, number, LabelText['part']]> = []
  const right: Array<[string, number, number, LabelText['part']]> = []
  if (atom.isotope !== null) left.push([String(atom.isotope), SMALL_SIZE, -5, 'isotope'])
  if (atom.hydrogens.mode !== 'inferred') {
    const count = atom.hydrogens.mode === 'unknown' ? '?' : atom.hydrogens.count === 1 ? '' : String(atom.hydrogens.count)
    const hydrogens: Array<[string, number, number, LabelText['part']]> = [['H', LABEL_SIZE, 0, 'hydrogens']]
    if (count) hydrogens.push([count, SMALL_SIZE, 4, 'hydrogen-count'])
    if (hydrogensLeft) left.push(...hydrogens.reverse())
    else right.push(...hydrogens)
  }
  const charge = chargeText(atom.charge)
  if (charge) right.push([charge, SMALL_SIZE, -5, 'charge'])

  const before: LabelText[] = []
  let leftEdge = -symbolWidth / 2
  for (const [text, size, dy, part] of left) {
    leftEdge -= textWidth(text, size)
    before.unshift(run(text, size, leftEdge, dy, part))
  }
  const texts: LabelText[] = [...before, run(symbol, LABEL_SIZE, -symbolWidth / 2, 0, 'symbol')]
  let rightEdge = symbolWidth / 2
  for (const [text, size, dy, part] of right) {
    texts.push(run(text, size, rightEdge, dy, part))
    rightEdge += textWidth(text, size)
  }

  const top = -LABEL_SIZE * HALF_HEIGHT
  const dotY = top - DOT_RADIUS - 1
  const dots: LabelDot[] = []
  const marks: AtomLabel['marks'] = []
  switch (atom.radical) {
    case 'none':
      break
    case 'doublet':
      dots.push({ dx: 0, dy: dotY, hollow: false })
      break
    case 'singlet':
      dots.push({ dx: -2.4, dy: dotY, hollow: false }, { dx: 2.4, dy: dotY, hollow: false })
      break
    case 'triplet':
      dots.push({ dx: -2.4, dy: dotY, hollow: true }, { dx: 2.4, dy: dotY, hollow: true })
      break
    case 'unknown':
      dots.push({ dx: -2, dy: dotY, hollow: false })
      marks.push({ ...run('?', MARK_SIZE, 0.5, dotY, 'symbol'), mark: 'radical' })
      break
    default: {
      const unhandled: never = atom.radical
      return unhandled
    }
  }
  if (atom.attachment_points.length > 0) {
    const text = `ap${atom.attachment_points.join(',')}`
    const width = textWidth(text, MARK_SIZE)
    const below = LABEL_SIZE * HALF_HEIGHT + MARK_SIZE * HALF_HEIGHT
    marks.push({ ...run(text, MARK_SIZE, -width / 2, below, 'symbol'), mark: 'attachment' })
  }

  const dotBoxes = dots.map((dot) => ({
    x: dot.dx - DOT_RADIUS - 0.5,
    y: dot.dy - DOT_RADIUS - 0.5,
    width: 2 * DOT_RADIUS + 1,
    height: 2 * DOT_RADIUS + 1,
  }))
  const content = union([...texts, ...marks].map(boxOf).concat(dotBoxes))
  const box = {
    x: content.x - PADDING,
    y: content.y - PADDING,
    width: content.width + 2 * PADDING,
    height: content.height + 2 * PADDING,
  }
  const what = atom.kind === 'query' && atom.query ? `query atom ${queryDescription(atom.query)}` : atom.element
  const name = [`Atom ${atom.id}: ${what}`, ...statesDescription(atom)].join('; ')
  return { texts, marks, dots, box, name }
}

export const DOT_SIZE = DOT_RADIUS

/** `box`, relative to an atom, placed at the atom's position. */
export function placed(box: Box, at: { x: number; y: number }): Box {
  return { x: box.x + at.x, y: box.y + at.y, width: box.width, height: box.height }
}
