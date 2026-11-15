import type { BondView, Position } from '@/lib/document-contract'

/** Proposed positions for one drawing instance, drawn over it and never part of the document. */
export interface LayoutGhost {
  instance: string
  positions: ReadonlyMap<string, Position>
}

/** One ghost line for a bond. A line has no direction: `from` is the begin end, `to` the end end. */
export interface GhostSegment {
  bond: string
  from: Position
  to: Position
}

/**
 * The bonds to draw as ghost lines, in the order of `bonds`: those with at least one end in
 * `positions`. An end outside `positions` is a fixed atom, placed at `drawn(atom)`. A bond whose
 * fixed end `drawn` cannot place (null) is left out.
 */
export function layoutGhostSegments(
  bonds: readonly BondView[],
  positions: ReadonlyMap<string, Position>,
  drawn: (atom: string) => Position | null,
): GhostSegment[] {
  const segments: GhostSegment[] = []
  for (const bond of bonds) {
    if (!positions.has(bond.begin) && !positions.has(bond.end)) continue
    const from = positions.get(bond.begin) ?? drawn(bond.begin)
    const to = positions.get(bond.end) ?? drawn(bond.end)
    if (from === null || to === null) continue
    segments.push({ bond: bond.id, from, to })
  }
  return segments
}
