/**
 * Gesture commands are built from the drawing on screen and sent when the request queue
 * reaches them. By then an earlier request (an undo, say) may have removed what they name.
 */
import type { Command, DocumentView } from '@/lib/document-contract'

function referencedIds(command: Command): string[] {
  switch (command.op) {
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
      return []
    case 'add_instance':
      return command.source === undefined ? [] : [command.source]
    case 'move_instance':
    case 'delete_instance':
      return [command.instance]
    case 'add_graphic':
      return command.parent === undefined ? [] : [command.parent]
    case 'group':
    case 'delete_objects':
    case 'move_objects':
      return command.objects
    case 'ungroup':
      return [command.group]
    case 'reorder':
      return [command.object]
    case 'add_reference':
    case 'remove_reference':
      return [command.source, command.target.id]
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
