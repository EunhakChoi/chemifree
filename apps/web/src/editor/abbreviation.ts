/** Presentation of an existing Et graph; this projection never becomes document state. */
import type { AbbreviationView, DocumentView } from '@/lib/document-contract'
import { textWidth, type AtomLabel, type LabelStyle } from '@/editor/label'

export function abbreviationsIn(document: DocumentView, instance: string | null): AbbreviationView[] {
  return (document.abbreviations ?? []).filter((item) => item.instance === instance)
}

export function compactView(document: DocumentView, instance: string | null): DocumentView {
  const hidden = new Set(abbreviationsIn(document, instance).map((item) => item.terminal))
  return {
    ...document,
    atoms: document.atoms.filter((atom) => !hidden.has(atom.id)),
    bonds: document.bonds.filter((bond) => !hidden.has(bond.begin) && !hidden.has(bond.end)),
  }
}

/** The visible label selects the retained pair and its induced internal bond. */
export function abbreviationTargets(document: DocumentView, instance: string | null, anchor: string): string[] {
  const item = abbreviationsIn(document, instance).find((item) => item.anchor === anchor)
  if (!item) return [anchor]
  const pair = [item.anchor, item.terminal]
  return [...pair, ...document.bonds.filter((bond) => pair.includes(bond.begin) && pair.includes(bond.end)).map((bond) => bond.id)]
}

export function ethylLabel(style: LabelStyle): AtomLabel {
  const width = textWidth('Et', style.size, style.font)
  const box = { x: -width / 2 - 1, y: -style.size * 0.65, width: width + 2, height: style.size * 1.3 }
  return {
    chunks: [{ role: 'text', anchor: 'middle', x: 0, y: 0,
      runs: [{ text: 'Et', size: style.size, shift: 0, part: 'symbol', weight: 500 }], box }],
    marks: [], dots: [], box, font: style.font,
  }
}
