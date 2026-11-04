import type { GroupMark } from '@/editor/stereo-groups'
import type { Position } from '@/lib/document-contract'

/**
 * The group label of each member of an enhanced stereo group: `abs`, or `OR 2`, `AND 0`. An
 * overlay for the person looking, drawn only while the Stereo panel's Group labels switch is on
 * (a client choice, not saved). It is not part of the drawing: `data-editor-overlay` lets an
 * export leave it out, it takes no pointer events, and each label is a named image. The point
 * `at` is where `groupLabelAt` puts its center. There is no color coding: the label's text is
 * all it says, and it states no ratio.
 */
export function GroupLabelLayer({ labels }: { labels: ReadonlyArray<{ label: GroupMark; at: Position }> }) {
  if (labels.length === 0) return null
  return (
    <g className="pointer-events-none" data-editor-overlay="">
      {labels.map(({ label, at }) => (
        <text
          key={label.atom}
          role="img"
          aria-label={label.name}
          data-group-label={label.text}
          data-group-atom={label.atom}
          x={at.x}
          y={at.y}
          fontSize={10}
          fontWeight={600}
          textAnchor="middle"
          dominantBaseline="central"
          strokeWidth={3}
          strokeLinejoin="round"
          paintOrder="stroke"
          className="fill-muted-foreground stroke-card"
        >
          {label.text}
        </text>
      ))}
    </g>
  )
}
