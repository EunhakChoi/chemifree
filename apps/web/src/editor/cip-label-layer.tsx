import type { CipMark } from '@/editor/cip-labels'
import type { Position } from '@/lib/document-contract'

/**
 * The CIP labels of one placement: the descriptor of each assigned item in parentheses. An
 * overlay for the person looking (the point `at` is where `labelAt` puts the label's center; its placement and the text form are Chemifree choices, not legacy behavior, and no marker is drawn for an unspecified center), not part of the drawing: `data-editor-overlay` lets an export
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
          x={at.x}
          y={at.y}
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
