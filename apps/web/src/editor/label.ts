/**
 * The layout of an atom's label: its text (element or query label, stated or unknown
 * hydrogens, charge, isotope), its state marks (radical dots, attachment points), where each
 * goes relative to the atom, and one box that encloses all of them. Bond trimming, the canvas
 * view box, and hit testing all use that box, so nothing is drawn outside the space they
 * reserve.
 *
 * Text is drawn at its natural width: glyphs are never stretched or squeezed. Each run of
 * text is anchored where it meets the symbol (the symbol centred on the atom, what follows it
 * starting at the symbol's reserved right edge, what precedes it ending at the reserved left
 * edge), so runs cannot overlap or drift into each other whatever font renders them. The box
 * reserves an upper bound of each glyph's advance (see ADVANCE), so the drawn text lies inside it.
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

export type LabelPart = 'isotope' | 'symbol' | 'hydrogens' | 'hydrogen-count' | 'charge' | 'attachment' | 'radical'

/** One run of text within a chunk; `shift` is its vertical middle relative to the chunk's line. */
export interface LabelRun {
  text: string
  size: number
  shift: number
  part: LabelPart
  weight: 400 | 500
}

/**
 * Runs drawn as one text element that flows from its anchor: `start` begins at `x`, `middle`
 * is centred on it, and `end` ends at it. `x`, `y` are relative to the atom; `box` is an upper
 * bound of where the runs are drawn.
 */
export interface LabelChunk {
  role: 'before' | 'symbol' | 'after' | 'attachment' | 'radical'
  anchor: 'start' | 'middle' | 'end'
  x: number
  y: number
  runs: LabelRun[]
  box: Box
}

/** A dot of a radical mark, relative to the atom; a hollow one is drawn as a ring. */
export interface LabelDot {
  dx: number
  dy: number
  hollow: boolean
}

export interface AtomLabel {
  /** Text chunks in reading order: what precedes the symbol, the symbol, what follows it. */
  chunks: LabelChunk[]
  /** Small text marks: the attachment points, and a radical stated as unknown. */
  marks: LabelChunk[]
  dots: LabelDot[]
  /** Encloses every chunk, mark, and dot, relative to the atom. */
  box: Box
}

export const LABEL_SIZE = 12
const SMALL_SIZE = 8
const MARK_SIZE = 7
/**
 * Half the height a run of text may take around its middle, as a share of its font size. Text
 * drawn with a central baseline measures at most 0.655 of its size either way in the bundled
 * font and the generic sans-serif fallback (Chromium); this keeps a margin.
 */
const HALF_HEIGHT = 0.7
/**
 * How far a run's drawn box may reach past its advance at either end, as a share of its size
 * (0.03 measured before the start and 0.02 past the end).
 */
const OVERHANG = 0.04
const DOT_RADIUS = 1.4
const PADDING = 1
/** List queries name at most this many elements on the canvas. */
export const SHOWN_QUERY_ELEMENTS = 3

/**
 * Upper bounds of each character's advance, as a share of the font size: the larger of the
 * bundled font (Geist, weights 400 and 500) and the generic sans-serif fallback shown while it
 * loads, measured in Chromium, plus 2%, rounded up. A character not listed counts as 1.1.
 */
const ADVANCE: ReadonlyMap<string, number> = new Map(
  (
    [
      ['i', 0.27],
      ['Ijl!,', 0.29],
      ['[]', 0.37],
      ['r', 0.4],
      ['ft', 0.42],
      ['*', 0.44],
      ['s', 0.56],
      ['yz17', 0.57],
      ['cv', 0.58],
      ['a?', 0.59],
      ['Le+−', 0.6],
      ['hnou', 0.61],
      ['6', 0.62],
      ['FJTZbdgkpqx9', 0.63],
      ['8', 0.64],
      ['234', 0.65],
      ['5', 0.66],
      ['EKPSXY0', 0.69],
      ['ABV', 0.71],
      ['CDHRU', 0.74],
      ['N', 0.77],
      ['GOQ', 0.8],
      ['w', 0.85],
      ['Mm', 0.91],
      ['W', 0.99],
      ['…', 1.02],
    ] as const
  ).flatMap(([characters, advance]) => [...characters].map((character): [string, number] => [character, advance])),
)
const UNKNOWN_ADVANCE = 1.1

/** An upper bound of the advance of `text` drawn at `size`. */
export function textWidth(text: string, size: number) {
  return [...text].reduce((total, character) => total + (ADVANCE.get(character) ?? UNKNOWN_ADVANCE) * size, 0)
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
 * The atom's accessible name: what it is, with the full query expression, and its recorded
 * states. `placement` names the drawing when the document is drawn more than once.
 */
export function atomName(atom: AtomView, placement: string | null = null) {
  const what = atom.kind === 'query' && atom.query ? `query atom ${queryDescription(atom.query)}` : atom.element
  const where = placement === null ? '' : ` in ${placement}`
  return [`Atom ${atom.id}${where}: ${what}`, ...statesDescription(atom)].join('; ')
}

/**
 * A carbon is drawn as a bare vertex only while it is bonded and carries nothing recorded:
 * a charge, isotope, stated or unknown hydrogens, a radical, or attachment points label it.
 * Inferred hydrogens do not. Every other element and every query atom is labelled.
 */
export function showsLabel(atom: AtomView, degree: number) {
  return atom.kind === 'query' || atom.element !== 'C' || degree === 0 || statesDescription(atom).length > 0
}

/** The smallest box enclosing all of `boxes`. */
export function union(boxes: Box[]): Box {
  const left = Math.min(...boxes.map((box) => box.x))
  const top = Math.min(...boxes.map((box) => box.y))
  const right = Math.max(...boxes.map((box) => box.x + box.width))
  const bottom = Math.max(...boxes.map((box) => box.y + box.height))
  return { x: left, y: top, width: right - left, height: bottom - top }
}

const run = (text: string, size: number, shift: number, part: LabelPart, weight: 400 | 500 = 400): LabelRun => ({
  text,
  size,
  shift,
  part,
  weight,
})

/** A chunk of `runs` anchored at `x`, `y`, with the upper-bound box of what it draws. */
function chunk(role: LabelChunk['role'], anchor: LabelChunk['anchor'], x: number, y: number, runs: LabelRun[]): LabelChunk {
  const advance = runs.reduce((total, item) => total + textWidth(item.text, item.size), 0)
  const overhang = OVERHANG * Math.max(...runs.map((item) => item.size))
  const start = anchor === 'start' ? x : anchor === 'end' ? x - advance : x - advance / 2
  const top = Math.min(...runs.map((item) => item.shift - HALF_HEIGHT * item.size))
  const bottom = Math.max(...runs.map((item) => item.shift + HALF_HEIGHT * item.size))
  return {
    role,
    anchor,
    x,
    y,
    runs,
    box: { x: start - overhang, y: y + top, width: advance + 2 * overhang, height: bottom - top },
  }
}

/** Layouts already made, by everything that shapes one; bounded so it cannot grow without end. */
const layouts = new Map<string, AtomLabel | null>()
const MAX_LAYOUTS = 4096

/**
 * The complete label of `atom` with `degree` bonds, or null for a bare carbon vertex. The
 * symbol is centred on the atom. Stated or unknown hydrogens go beside it with their count as
 * a subscript ("?" when unknown): after it, or before it ("H3C") when `hydrogensLeft` because
 * the atom's bonds lean right. The isotope is a superscript before the symbol and the charge
 * one after it. Radical dots sit above the symbol and attachment points below it.
 *
 * A layout depends only on the atom's recorded states, whether it is bonded, and the side, so
 * atoms alike share one (frozen) layout object.
 */
export function atomLabel(atom: AtomView, degree: number, hydrogensLeft = false): AtomLabel | null {
  const key = JSON.stringify([
    atom.kind,
    atom.element,
    atom.charge,
    atom.isotope,
    atom.hydrogens,
    atom.radical,
    atom.attachment_points,
    degree === 0,
    hydrogensLeft,
  ])
  const known = layouts.get(key)
  if (known !== undefined) return known
  const made = layout(atom, degree, hydrogensLeft)
  if (layouts.size >= MAX_LAYOUTS) layouts.clear()
  layouts.set(key, made)
  return made
}

function layout(atom: AtomView, degree: number, hydrogensLeft: boolean): AtomLabel | null {
  if (!showsLabel(atom, degree)) return null
  const symbolText = atom.kind === 'query' && atom.query ? queryLabel(atom.query) : atom.element
  const symbol = chunk('symbol', 'middle', 0, 0, [run(symbolText, LABEL_SIZE, 0, 'symbol', 500)])
  const before: LabelRun[] = []
  const after: LabelRun[] = []
  const hydrogens: LabelRun[] = []
  if (atom.hydrogens.mode !== 'inferred') {
    hydrogens.push(run('H', LABEL_SIZE, 0, 'hydrogens', 500))
    const count = atom.hydrogens.mode === 'unknown' ? '?' : atom.hydrogens.count === 1 ? '' : String(atom.hydrogens.count)
    if (count) hydrogens.push(run(count, SMALL_SIZE, 4, 'hydrogen-count'))
  }
  if (hydrogensLeft) before.push(...hydrogens)
  if (atom.isotope !== null) before.push(run(String(atom.isotope), SMALL_SIZE, -5, 'isotope'))
  if (!hydrogensLeft) after.push(...hydrogens)
  const charge = chargeText(atom.charge)
  if (charge) after.push(run(charge, SMALL_SIZE, -5, 'charge'))

  // What precedes the symbol ends at its reserved left edge; what follows starts at its right.
  const chunks: LabelChunk[] = []
  if (before.length > 0) chunks.push(chunk('before', 'end', symbol.box.x, 0, before))
  chunks.push(symbol)
  if (after.length > 0) chunks.push(chunk('after', 'start', symbol.box.x + symbol.box.width, 0, after))

  const dotY = symbol.box.y - DOT_RADIUS - 1
  const dots: LabelDot[] = []
  const marks: LabelChunk[] = []
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
      marks.push(chunk('radical', 'start', 0.5, dotY, [run('?', MARK_SIZE, 0, 'radical')]))
      break
    default: {
      const unhandled: never = atom.radical
      return unhandled
    }
  }
  if (atom.attachment_points.length > 0) {
    const below = symbol.box.y + symbol.box.height + HALF_HEIGHT * MARK_SIZE
    marks.push(chunk('attachment', 'middle', 0, below, [run(`ap${atom.attachment_points.join(',')}`, MARK_SIZE, 0, 'attachment')]))
  }

  const dotBoxes = dots.map((dot) => ({
    x: dot.dx - DOT_RADIUS - 0.5,
    y: dot.dy - DOT_RADIUS - 0.5,
    width: 2 * DOT_RADIUS + 1,
    height: 2 * DOT_RADIUS + 1,
  }))
  const content = union([...chunks, ...marks].map((item) => item.box).concat(dotBoxes))
  const box = {
    x: content.x - PADDING,
    y: content.y - PADDING,
    width: content.width + 2 * PADDING,
    height: content.height + 2 * PADDING,
  }
  return Object.freeze({ chunks, marks, dots, box })
}

/** The label's text in reading order, marks left out. */
export function labelText(label: AtomLabel) {
  return label.chunks.flatMap((item) => item.runs.map((part) => part.text))
}

export const DOT_SIZE = DOT_RADIUS

/** `box`, relative to an atom, placed at the atom's position. */
export function placed(box: Box, at: { x: number; y: number }): Box {
  return { x: box.x + at.x, y: box.y + at.y, width: box.width, height: box.height }
}
