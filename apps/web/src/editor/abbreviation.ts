/** Presentation of an existing Et graph; this projection never becomes document state. */
import type { AbbreviationView, DocumentView } from '@/lib/document-contract'
import { compactLabel, type AtomLabel, type LabelStyle } from '@/editor/label'

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
  return compactLabel('Et', style)
}
