/**
 * The layout of an atom's label: its text (element or query label, stated or unknown
 * hydrogens, charge, isotope), its state marks (radical dots, attachment points), where each
 * goes relative to the atom, and one box that encloses all of them. Bond trimming, the canvas
 * view box, and hit testing all use that box, so nothing is drawn outside the space they
 * reserve.
 *
 * Text is drawn at its natural width: glyphs are never stretched or squeezed. The symbol and
 * what precedes and follows it are one chunk of text, so they flow against each other and
 * cannot overlap or drift apart whatever font renders them; the chunk is centred so that the
 * symbol's reserved width is centred on the atom. Normal uses the 400/500 advance bounds
 * in METRICS; bold uses actual SVG advances and glyph ink measured by the canvas.
 *
 * The document's settings choose label font, size, weight and which carbons are labelled
 * (LabelStyle). A layout is made at the built-in size and every length in it, boxes included,
 * scaled by the label size, so bond trimming, the view box, and hit areas follow one layout.
 *
 * Nothing here computes chemistry: inferred hydrogens are not counted and add nothing.
 */
import type { AtomView, CarbonLabels, DocumentSettings, LabelFont, LabelWeight, QueryAtom } from '@/lib/document-contract'

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
  weight: 400 | 500 | 700
}

/**
 * Runs drawn as one text element that flows from its anchor: `start` begins at `x`, `middle`
 * is centred on it, and `end` ends at it. `x`, `y` are relative to the atom; `box` is an upper
 * bound of where the runs are drawn.
 */
export interface LabelChunk {
  role: 'text' | 'attachment' | 'radical'
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
  radius: number
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
  /** The font every chunk and mark is drawn in. */
  font: LabelFont
}

/** How a document's labels are drawn: from its label font, label size, and carbon labels. */
export interface LabelStyle {
  font: LabelFont
  /** The symbol's font size, in pt; everything else in a label scales with it. */
  size: number
  carbons: CarbonLabels
  weight: LabelWeight
  /** Browser measurements of the actual 700 text runs, in document units. */
  metrics?: LabelMetrics
}

/** The built-in label size, at which a layout is made before it is scaled. */
export const LABEL_SIZE = 12
export const DEFAULT_LABEL_STYLE: Readonly<LabelStyle> = Object.freeze({ font: 'sans-serif', size: LABEL_SIZE, carbons: 'default', weight: 'normal' })

export function labelStyleOf(settings: Readonly<DocumentSettings>, metrics?: LabelMetrics): LabelStyle {
  return { font: settings.label_font, size: settings.label_size_pt, carbons: settings.carbon_labels,
    weight: settings.label_weight, ...(metrics ? { metrics } : {}) }
}

const SMALL_SIZE = 8
const MARK_SIZE = 7
/**
 * Half the height a run of text may take around its middle, as a share of its font size. Text
 * drawn with a central baseline measures at most 0.655 of its size either way in the bundled
 * font and the generic sans-serif fallback (Chromium), and at most 0.53 in the serif and
 * monospace fonts measured for METRICS; this keeps a margin.
 */
const HALF_HEIGHT = 0.7
const DOT_RADIUS = 1.4
const PADDING = 1
/** List queries name at most this many elements on the canvas. */
export const SHOWN_QUERY_ELEMENTS = 3

/** Characters by the upper bound of their advance, as a share of the font size. */
type AdvanceTable = ReadonlyArray<readonly [string, number]>

const advances = (table: AdvanceTable): ReadonlyMap<string, number> =>
  new Map(table.flatMap(([characters, advance]) => [...characters].map((character): [string, number] => [character, advance])))

/**
 * Per label font: upper bounds of each character's advance, as a share of the font size, and
 * how far a run's drawn box may reach past its advance at either end (`overhang`). A character
 * not listed counts as 1.1.
 *
 * `sans-serif` is the bundled font (Geist, weights 400 and 500) and the generic sans-serif
 * fallback shown while it loads, measured in Chromium, plus 2%, rounded up (0.03 measured before
 * the start and 0.02 past the end). `serif` and `monospace` are the generic families: the
 * largest of Chromium's generic family and DejaVu Serif, Liberation Serif, and Nimbus Roman
 * (serif), or DejaVu Sans Mono, Liberation Mono, Courier, and Nimbus Mono PS (monospace), at
 * weights 400 and 500, plus 2%, rounded up; overhang up to 0.1 was measured (DejaVu Serif).
 * Another system's generic family may be wider; text drawn in it may then reach past the box.
 */
const METRICS: Readonly<Record<LabelFont, { advance: ReadonlyMap<string, number>; overhang: number }>> = {
  'sans-serif': {
    overhang: 0.04,
    advance: advances([
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
    ]),
  },
  serif: {
    overhang: 0.11,
    advance: advances([
      ['j', 0.32],
      ['il,', 0.33],
      ['f', 0.38],
      ['[]', 0.4],
      ['I!tJ', 0.41],
      ['r', 0.49],
      ['*', 0.51],
      ['s', 0.53],
      ['z', 0.54],
      ['?', 0.55],
      ['ycvx', 0.58],
      ['ae', 0.61],
      ['ok', 0.62],
      ['0123456789', 0.65],
      ['hnubdgpq', 0.66],
      ['L', 0.68],
      ['TP', 0.69],
      ['S', 0.7],
      ['FZ', 0.71],
      ['XYAV', 0.74],
      ['EB', 0.75],
      ['KR', 0.77],
      ['C', 0.79],
      ['DG', 0.82],
      ['OQ', 0.84],
      ['+−U', 0.86],
      ['w', 0.88],
      ['H', 0.89],
      ['N', 0.9],
      ['m', 0.97],
      ['…', 1.02],
      ['MW', 1.05],
    ]),
  },
  monospace: {
    overhang: 0.04,
    advance: advances([['0123456789iIjl!,[]rft*syzcva?Le+−hnouFJTZbdgkpqxEKPSXYABVCDHRUNGOQwMmW…', 0.62]]),
  },
}
const UNKNOWN_ADVANCE = 1.1

interface LabelMetric {
  advance: number
  overhang: number
  top: number
  bottom: number
}
export type LabelMetrics = ReadonlyMap<string, LabelMetric>

function labelMetricKey(text: string, size: number, font: LabelFont): string {
  return JSON.stringify([text, size, font])
}

/** Natural 700 advances and glyph overhang measured from the SVG that is actually painted. */
export function measureBoldLabels(svg: SVGSVGElement): LabelMetrics {
  const metrics = new Map<string, LabelMetric>()
  for (const group of svg.querySelectorAll<SVGGElement>('[data-labels]')) {
    const font = group.getAttribute('data-labels')
    if (font !== 'sans-serif' && font !== 'serif' && font !== 'monospace') continue
    for (const span of group.querySelectorAll<SVGTSpanElement>('tspan[font-weight="700"]')) {
      const text = span.textContent ?? ''
      if (!text) continue
      const size = Number(span.getAttribute('font-size'))
      const advance = span.getComputedTextLength()
      const ink = span.getBBox()
      const start = span.getStartPositionOfChar(0).x
      // Keep a small rounding/hinting margin at either viewport scale. No textLength/stretch.
      const overhang = Math.max(0, start - ink.x, ink.x + ink.width - start - advance) + size * 0.05 + 0.5
      const parent = span.parentElement
      let middle = Number(parent?.getAttribute('y') ?? 0)
      for (const sibling of Array.from(parent?.children ?? [])) {
        middle += Number(sibling.getAttribute('dy') ?? 0)
        if (sibling === span) break
      }
      metrics.set(labelMetricKey(text, size, font), {
        advance, overhang,
        top: ink.y - middle - size * 0.05 - 0.5,
        bottom: ink.y + ink.height - middle + size * 0.05 + 0.5,
      })
    }
  }
  return metrics
}

function runMetric(item: LabelRun, font: LabelFont, metrics?: LabelMetrics, scale = 1): LabelMetric {
  if (item.weight === 700) {
    const measured = metrics?.get(labelMetricKey(item.text, item.size * scale, font))
    if (measured) return { advance: measured.advance / scale, overhang: measured.overhang / scale, top: measured.top / scale, bottom: measured.bottom / scale }
    // Provisional bounds until the mounted SVG is measured; never used as 700 font evidence.
    return { advance: textWidth(item.text, item.size, font) * 1.5, overhang: item.size * 0.2, top: -item.size, bottom: item.size }
  }
  return { advance: textWidth(item.text, item.size, font), overhang: METRICS[font].overhang * item.size, top: -HALF_HEIGHT * item.size, bottom: HALF_HEIGHT * item.size }
}

/** An upper bound of the advance of `text` drawn at `size` in `font`. */
export function textWidth(text: string, size: number, font: LabelFont = 'sans-serif') {
  const { advance } = METRICS[font]
  return [...text].reduce((total, character) => total + (advance.get(character) ?? UNKNOWN_ADVANCE) * size, 0)
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
 * With the `default` carbon labels, a carbon is drawn as a bare vertex only while it is bonded
 * and carries nothing recorded: a charge, isotope, stated or unknown hydrogens, a radical, or
 * attachment points label it. Inferred hydrogens do not. Every other element and every query
 * atom is labelled, and with `all` every carbon too.
 */
export function showsLabel(atom: AtomView, degree: number, carbons: CarbonLabels = 'default') {
  return (
    carbons === 'all' ||
    atom.kind === 'query' ||
    atom.element !== 'C' ||
    degree === 0 ||
    statesDescription(atom).length > 0
  )
}

/** The smallest box enclosing all of `boxes`. */
export function union(boxes: Box[]): Box {
  const left = Math.min(...boxes.map((box) => box.x))
  const top = Math.min(...boxes.map((box) => box.y))
  const right = Math.max(...boxes.map((box) => box.x + box.width))
  const bottom = Math.max(...boxes.map((box) => box.y + box.height))
  return { x: left, y: top, width: right - left, height: bottom - top }
}

const run = (text: string, size: number, shift: number, part: LabelPart, weight: 400 | 500 | 700 = 400): LabelRun => ({
  text,
  size,
  shift,
  part,
  weight,
})

/** A chunk of `runs` anchored at `x`, `y`, with the upper-bound box of what it draws in `font`. */
function chunk(
  font: LabelFont,
  role: LabelChunk['role'],
  anchor: LabelChunk['anchor'],
  x: number,
  y: number,
  runs: LabelRun[],
  metrics?: LabelMetrics,
  scale = 1,
): LabelChunk {
  const measured = runs.map((item) => runMetric(item, font, metrics, scale))
  const advance = measured.reduce((total, item) => total + item.advance, 0)
  const overhang = Math.max(...measured.map((item) => item.overhang))
  const start = anchor === 'start' ? x : anchor === 'end' ? x - advance : x - advance / 2
  const top = Math.min(...runs.map((item, index) => item.shift + measured[index].top))
  const bottom = Math.max(...runs.map((item, index) => item.shift + measured[index].bottom))
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
 * The complete label of `atom` with `degree` bonds in `style`, or null for a bare carbon
 * vertex. The symbol is centred on the atom. Stated or unknown hydrogens go beside it with
 * their count as a subscript ("?" when unknown): after it, or before it ("H3C") when
 * `hydrogensLeft` because the atom's bonds lean right. The isotope is a superscript before the
 * symbol and the charge one after it. Radical dots sit above the symbol and attachment points
 * below it.
 *
 * A layout depends only on the atom's recorded states, whether it is bonded, the side, and the
 * style, so atoms alike share one (frozen) layout object.
 */
export function atomLabel(
  atom: AtomView,
  degree: number,
  hydrogensLeft = false,
  style: Readonly<LabelStyle> = DEFAULT_LABEL_STYLE,
): AtomLabel | null {
  if (!showsLabel(atom, degree, style.carbons)) return null
  const key = JSON.stringify([
    atom.kind,
    atom.query,
    atom.element,
    atom.charge,
    atom.isotope,
    atom.hydrogens,
    atom.radical,
    atom.attachment_points,
    hydrogensLeft,
    style.font,
    style.size,
  ])
  const known = style.weight === 'bold' ? undefined : layouts.get(key)
  if (known !== undefined) return known
  const made = scaled(layout(atom, hydrogensLeft, style), style.size / LABEL_SIZE)
  if (layouts.size >= MAX_LAYOUTS) layouts.clear()
  if (style.weight !== 'bold') layouts.set(key, made)
  return made
}

/** `label` with every length in it multiplied by `factor`, frozen. */
function scaled(label: AtomLabel, factor: number): AtomLabel {
  const box = (item: Box): Box => ({
    x: item.x * factor,
    y: item.y * factor,
    width: item.width * factor,
    height: item.height * factor,
  })
  const text = (item: LabelChunk): LabelChunk => ({
    ...item,
    x: item.x * factor,
    y: item.y * factor,
    runs: item.runs.map((part) => ({ ...part, size: part.size * factor, shift: part.shift * factor })),
    box: box(item.box),
  })
  return Object.freeze({
    chunks: label.chunks.map(text),
    marks: label.marks.map(text),
    dots: label.dots.map((dot) => ({ ...dot, dx: dot.dx * factor, dy: dot.dy * factor, radius: dot.radius * factor })),
    box: box(label.box),
    font: label.font,
  })
}

/** The label of a labelled atom at the built-in size. */
function layout(atom: AtomView, hydrogensLeft: boolean, style: Readonly<LabelStyle>): AtomLabel {
  const { font, metrics } = style
  const scale = style.size / LABEL_SIZE
  const chemicalRun = (text: string, size: number, shift: number, part: LabelPart, weight: 400 | 500 = 400) =>
    run(text, size, shift, part, style.weight === 'bold' ? 700 : weight)
  const chunkOf = (role: LabelChunk['role'], anchor: LabelChunk['anchor'], x: number, y: number, runs: LabelRun[]) =>
    chunk(font, role, anchor, x, y, runs, metrics, scale)
  const symbolText = atom.kind === 'query' && atom.query ? queryLabel(atom.query) : atom.element
  const symbolRun = chemicalRun(symbolText, LABEL_SIZE, 0, 'symbol', 500)
  // The symbol's reserved box, centred on the atom: radical dots go above it, marks below.
  const symbol = chunkOf('text', 'middle', 0, 0, [symbolRun])
  const before: LabelRun[] = []
  const after: LabelRun[] = []
  const hydrogens: LabelRun[] = []
  if (atom.hydrogens.mode !== 'inferred') {
    hydrogens.push(chemicalRun('H', LABEL_SIZE, 0, 'hydrogens', 500))
    const count = atom.hydrogens.mode === 'unknown' ? '?' : atom.hydrogens.count === 1 ? '' : String(atom.hydrogens.count)
    if (count) hydrogens.push(chemicalRun(count, SMALL_SIZE, 4, 'hydrogen-count'))
  }
  if (hydrogensLeft) before.push(...hydrogens)
  if (atom.isotope !== null) before.push(chemicalRun(String(atom.isotope), SMALL_SIZE, -5, 'isotope'))
  if (!hydrogensLeft) after.push(...hydrogens)
  const charge = chargeText(atom.charge)
  if (charge) after.push(chemicalRun(charge, SMALL_SIZE, -5, 'charge'))

  // One chunk of text, so what precedes and follows the symbol is drawn against it at natural
  // widths. Centred where the reserved widths centre the symbol's on the atom: the symbol is
  // then off the atom by half the difference of the reserved and natural widths of what is
  // beside it (a fraction of a point), and the text lies inside the reserved widths.
  const width = (runs: LabelRun[]) => runs.reduce((total, item) => total + runMetric(item, font, metrics, scale).advance, 0)
  const chunks = [chunkOf('text', 'middle', (width(after) - width(before)) / 2, 0, [...before, symbolRun, ...after])]

  const dotY = symbol.box.y - DOT_RADIUS - 1
  const dots: LabelDot[] = []
  const marks: LabelChunk[] = []
  switch (atom.radical) {
    case 'none':
      break
    case 'doublet':
      dots.push({ dx: 0, dy: dotY, radius: DOT_RADIUS, hollow: false })
      break
    case 'singlet':
      dots.push({ dx: -2.4, dy: dotY, radius: DOT_RADIUS, hollow: false }, { dx: 2.4, dy: dotY, radius: DOT_RADIUS, hollow: false })
      break
    case 'triplet':
      dots.push({ dx: -2.4, dy: dotY, radius: DOT_RADIUS, hollow: true }, { dx: 2.4, dy: dotY, radius: DOT_RADIUS, hollow: true })
      break
    case 'unknown':
      dots.push({ dx: -2, dy: dotY, radius: DOT_RADIUS, hollow: false })
      marks.push(chunkOf('radical', 'start', 0.5, dotY, [run('?', MARK_SIZE, 0, 'radical')]))
      break
    default: {
      const unhandled: never = atom.radical
      return unhandled
    }
  }
  if (atom.attachment_points.length > 0) {
    const below = symbol.box.y + symbol.box.height + HALF_HEIGHT * MARK_SIZE
    marks.push(chunkOf('attachment', 'middle', 0, below, [run(`ap${atom.attachment_points.join(',')}`, MARK_SIZE, 0, 'attachment')]))
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
  return { chunks, marks, dots, box, font }
}

/** Graph-preserving Et uses the same weight and geometry as a chemical symbol. */
export function compactLabel(text: string, style: Readonly<LabelStyle>): AtomLabel {
  if (style.weight !== 'bold') {
    const width = textWidth(text, style.size, style.font)
    const box = { x: -width / 2 - 1, y: -style.size * 0.65, width: width + 2, height: style.size * 1.3 }
    return { chunks: [{ role: 'text', anchor: 'middle', x: 0, y: 0, runs: [run(text, style.size, 0, 'symbol', 500)], box }], marks: [], dots: [], box, font: style.font }
  }
  const item = run(text, style.size, 0, 'symbol', 700)
  const content = chunk(style.font, 'text', 'middle', 0, 0, [item], style.metrics)
  const padding = style.size / LABEL_SIZE
  return {
    chunks: [content], marks: [], dots: [], font: style.font,
    box: { x: content.box.x - padding, y: content.box.y - padding, width: content.box.width + 2 * padding, height: content.box.height + 2 * padding },
  }
}

/** The label's text in reading order, marks left out. */
export function labelText(label: AtomLabel) {
  return label.chunks.flatMap((item) => item.runs.map((part) => part.text))
}

/** `box`, relative to an atom, placed at the atom's position. */
export function placed(box: Box, at: { x: number; y: number }): Box {
  return { x: box.x + at.x, y: box.y + at.y, width: box.width, height: box.height }
}
