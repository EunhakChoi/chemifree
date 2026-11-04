import type { CipMark } from '@/editor/cip-labels'
import type { Position } from '@/lib/document-contract'

/**
 * Offset (pt) of a label from its anchor: up and to the right of an atom, clear of the label of
 * the atom and of an agreement ring (radius 11); for a double bond the same offset from the
 * middle of the bond. A fixed offset is the whole placement rule: it can still touch a bond
 * line, a mark or another label in a crowded drawing. The placement, the text form (`(S)` for
 * an atom; `(E)` or `(Z)` for a bond, a product choice) and the absence of any marker for an
 * unspecified center are Chemifree's choices, not legacy behavior.
 */
export const LABEL_OFFSET = { x: 12, y: -12 }

/**
 * The CIP labels of one placement: the descriptor of each assigned item in parentheses. An
 * overlay for the person looking, not part of the drawing: `data-editor-overlay` lets an export
 * leave it out, it takes no pointer events, and each label is a named image whose name says it
 * is the label of the stored configuration. A halo in the card color keeps it legible over a line.
 */
export function CipLabelLayer({ labels }: { labels: ReadonlyArray<{ label: CipMark; at: Position }> }) {
  if (labels.length === 0) return null
  return (
    <g className="pointer-events-none" data-editor-overlay="">
      {labels.map(({ label, at }) => (
        <text
          key={`${label.item.kind}:${label.item.id}`}
          role="img"
          aria-label={label.name}
          data-cip-label={label.text}
          data-cip-item={label.item.id}
          x={at.x + LABEL_OFFSET.x}
          y={at.y + LABEL_OFFSET.y}
          fontSize={11}
          fontWeight={600}
          textAnchor="middle"
          dominantBaseline="central"
          strokeWidth={3}
          strokeLinejoin="round"
          paintOrder="stroke"
          className="fill-foreground stroke-card"
        >
          {label.text}
        </text>
      ))}
    </g>
  )
}
