/**
 * The sentence the layout panel shows for a `layout_unsupported` reason (CONTRACT.md "Chain and simple
 * ring layout"). Returns null for a reason this table does not know, so the caller shows the service's
 * own message instead. `limit` is the ring-size bound the service named, or null when it named none.
 */
export function layoutRefusalSentence(reason: string, limit: number | null): string | null {
  switch (reason) {
    case 'object_selection':
      return 'Graphics and placements are not laid out. Select atoms or bonds.'
    case 'ring_partly_in_layout_set':
      return 'A ring is only partly in the layout. Select the whole ring, or turn off "Move only the selected atoms".'
    case 'fused_ring_in_layout_set':
      return 'Fused and bridged rings are not laid out yet.'
    case 'spiro_ring_in_layout_set':
      return 'Spiro rings are not laid out yet.'
    case 'large_ring_in_layout_set':
      return `Rings of more than ${limit ?? 12} atoms are not laid out yet.`
    case 'stereo_in_layout':
      return 'Atoms or bonds with stated stereo or a wedge, hashed or wavy mark are not moved by this layout.'
    case 'display_in_layout_set':
      return 'Atoms shown as an abbreviation or fragment label are not laid out. Expand the label first.'
    case 'multiple_attachments':
      return 'A part of the selection is attached to the rest of the drawing by more than one bond.'
    case 'coincident_fixed_atoms':
      return 'Two fixed atoms lie at the same point next to the attachment atom.'
    case 'coordinate_out_of_range':
      return 'A proposed position lies outside the drawing limits.'
    default:
      return null
  }
}
