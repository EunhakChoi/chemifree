/** Presentation of retained graphs; this projection never becomes document state. */
import { fragmentDisplayRuns, type FragmentDisplayRun, type AbbreviationView, type DocumentView } from '@/lib/document-contract'
import { compactLabel, type AtomLabel, type LabelStyle } from '@/editor/label'

export function abbreviationsIn(document: DocumentView, instance: string | null): AbbreviationView[] {
  return (document.abbreviations ?? []).filter((item) => item.instance === instance)
}

export function compactDisplaysIn(document: DocumentView, instance: string | null): Array<{ anchor: string; members: string[]; text: string; ethyl: boolean; runs?: FragmentDisplayRun[] }> {
  return [
    ...abbreviationsIn(document, instance).map((item) => ({ anchor: item.anchor, members: [item.anchor, item.terminal], text: item.display_label, ethyl: true })),
    ...(document.fragment_displays ?? []).filter((item) => item.instance === instance).map((item) => ({ anchor: item.anchor, members: item.members, text: item.text, ethyl: false, runs: fragmentDisplayRuns(item) })),
  ]
}

export function compactView(document: DocumentView, instance: string | null): DocumentView {
  const hidden = new Set(compactDisplaysIn(document, instance).flatMap((item) => item.members.filter((id) => id !== item.anchor)))
  return {
    ...document,
    atoms: document.atoms.filter((atom) => !hidden.has(atom.id)),
    bonds: document.bonds.filter((bond) => !hidden.has(bond.begin) && !hidden.has(bond.end)),
  }
}

/** The visible label selects every retained member and induced internal bond. */
export function abbreviationTargets(document: DocumentView, instance: string | null, anchor: string): string[] {
  const item = compactDisplaysIn(document, instance).find((item) => item.anchor === anchor)
  if (!item) return [anchor]
  const pair = item.members
  return [...pair, ...document.bonds.filter((bond) => pair.includes(bond.begin) && pair.includes(bond.end)).map((bond) => bond.id)]
}

export function ethylLabel(style: LabelStyle, text = 'Et', measured = false): AtomLabel {
  return compactLabel(text, style, measured)
}
