import type { CaptionAlignment, CaptionRun, DocumentSettings, LabelFont } from '@/lib/document-contract'

/**
 * How one caption run is drawn, as plain numbers and keywords. scene-caption.tsx draws each run with
 * these values. The SVG export lays out the same rules in Rust (svg/caption.rs), and
 * scripts/svg-parity-client-check.mjs compares the two.
 */
export interface CaptionRunStyle {
  /** The generic family: the run's own font, else the label font. */
  family: LabelFont
  /** The glyph size in pt: the run's size, or two thirds of it for a subscript or superscript. */
  size: number
  /** The baseline offset from the caption's anchor in pt, positive downwards. */
  shift: number
  weight: CaptionRun['weight']
  style: CaptionRun['style']
  /** The stored colour, or null for the theme foreground. */
  color: string | null
  /** The stroke width of an outlined run, size / 16, or null for a filled run. */
  outline: number | null
}

const TEXT_ANCHORS: Readonly<Record<CaptionAlignment, 'start' | 'middle' | 'end'>> = { left: 'start', center: 'middle', right: 'end' }

/**
 * The drawing rules of one caption run. A run without its own size takes the label size. A subscript
 * sinks by a third of the run size, and a superscript rises by five twelfths of it.
 */
export function captionRunStyle(
  run: CaptionRun,
  settings: Pick<DocumentSettings, 'label_font' | 'label_size_pt'>,
): CaptionRunStyle {
  const runSize = run.size_pt ?? settings.label_size_pt
  const size = run.script === 'baseline' ? runSize : runSize * 2 / 3
  return {
    family: run.font ?? settings.label_font,
    size,
    shift: run.script === 'subscript' ? runSize / 3 : run.script === 'superscript' ? -runSize * 5 / 12 : 0,
    weight: run.weight,
    style: run.style,
    color: run.color ?? null,
    outline: run.outline ? size / 16 : null,
  }
}

/**
 * The SVG text-anchor that aligns a whole caption at its anchor, for the parity check
 * (scripts/svg-parity-client-check.mjs). The web never writes it: structure-canvas.tsx shifts a
 * caption's x by its measured advance instead.
 */
export function captionTextAnchor(alignment: CaptionAlignment): 'start' | 'middle' | 'end' {
  return TEXT_ANCHORS[alignment]
}
