/**
 * Gesture commands are built from the drawing on screen and sent when the request queue
 * reaches them. By then an earlier request (an undo, say) may have removed what they name.
 */
import type { Command, DocumentView } from '@/lib/document-contract'
import { IMPLICIT_HYDROGEN, LONE_PAIR } from '@/lib/stereo-contract'

/** The neighbors of a stereo command that are atoms: not the two words that name no atom. */
const namedAtoms = (neighbors: string[]) => neighbors.filter((id) => id !== IMPLICIT_HYDROGEN && id !== LONE_PAIR)

function referencedIds(command: Command): string[] {
  switch (command.op) {
    case 'contract_fragment':
      return [command.instance, command.anchor, ...command.members]
    case 'set_fragment_display_runs':
    case 'set_fragment_display_text':
    case 'expand_fragment':
      return [command.instance, command.anchor]
    case 'collapse_ethyl':
      return [command.instance, command.anchor, command.terminal]
    case 'update_abbreviation_label':
    case 'expand_ethyl':
      return [command.instance, command.anchor]
    case 'attach_ring':
      return [command.instance, command.atom]
    case 'insert_ring':
      return [command.instance]
    case 'add_atom': {
      const target = command.bond_to ? [command.bond_to.atom] : []
      return 'positions' in command ? [...target, ...Object.keys(command.positions)] : target
    }
    case 'add_bond':
      return [command.begin, command.end]
    case 'update_atom':
      return [command.atom]
    case 'move_atoms':
      return command.instance === undefined ? command.atoms : [...command.atoms, command.instance]
    case 'delete_atoms':
      return command.atoms
    case 'update_bond':
      return [command.bond]
    case 'delete_bonds':
      return command.bonds
    case 'rename':
    case 'update_settings':
      return []
    case 'add_instance':
      return command.source === undefined ? [] : [command.source]
    case 'move_instance':
    case 'delete_instance':
      return [command.instance]
    case 'set_graphic_line_width':
    case 'update_graphic_points':
    case 'update_caption':
      return [command.object]
    case 'add_caption':
    case 'add_graphic':
      return command.parent === undefined ? [] : [command.parent]
    case 'group':
    case 'delete_objects':
    case 'move_objects':
    case 'arrange_objects':
      return command.objects
    case 'move_selection':
    case 'transform_selection':
      return [...command.selection.objects, ...command.selection.atoms, ...command.selection.bonds, ...(command.instance === undefined ? [] : [command.instance])]
    case 'ungroup':
      return [command.group]
    case 'reorder':
      return [command.object]
    case 'add_reference':
    case 'remove_reference':
      return [command.source, command.target.id]
    case 'set_bond_mark':
      return [command.bond]
    case 'set_atom_configuration':
      return command.configuration === null ? [command.atom] : [command.atom, ...namedAtoms(command.configuration.neighbors)]
    case 'set_bond_configuration':
      return command.configuration === null || command.configuration.kind === 'unknown'
        ? [command.bond]
        : [command.bond, ...namedAtoms([command.configuration.begin_neighbor, command.configuration.end_neighbor])]
    case 'set_configuration_from_drawing':
      return [...(command.atoms ?? []), ...(command.bonds ?? []), ...(command.instance === undefined ? [] : [command.instance])]
    // Group commands name atoms of the document and group ids; this lists the ids of atoms, bonds and scene objects only.
    case 'create_stereo_group':
    case 'split_stereo_group':
    case 'add_stereo_group_members':
    case 'remove_stereo_group_members':
      return command.atoms
    case 'delete_stereo_group':
    case 'set_stereo_group_kind':
    case 'set_stereo_group_number':
    case 'merge_stereo_groups':
      return []
    default: {
      const unknown: never = command
      return unknown
    }
  }
}

/**
 * The commands if everything they name still exists and every new atom is placed in exactly
 * the scene instances there are now, otherwise null (the gesture lapses).
 */
export function stillApplicable(commands: Command[], document: DocumentView): Command[] | null {
  const instances = document.scene?.instances.map((instance) => instance.id) ?? []
  const objects = document.scene?.objects?.map((object) => object.id) ?? instances
  const existing = new Set([...document.atoms.map((atom) => atom.id), ...document.bonds.map((bond) => bond.id), ...objects])
  const placedEverywhere = (command: Command) => {
    if (command.op !== 'add_atom') return true
    if ('positions' in command) {
      const placed = Object.keys(command.positions)
      return placed.length === instances.length && placed.every((id) => instances.includes(id))
    }
    return instances.length <= 1
  }
  const valid = commands.every(
    (command) =>
      placedEverywhere(command) &&
      // `$N` names an atom created earlier in the same transaction.
      referencedIds(command).every((id) => id.startsWith('$') || existing.has(id)),
  )
  return valid ? commands : null
}
