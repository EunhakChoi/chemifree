/**
 * Gesture commands are built from the drawing on screen and sent when the request queue
 * reaches them. By then an earlier request (an undo, say) may have removed what they name.
 */
import type { Command, DocumentView } from '@/lib/document-contract'

function referencedIds(command: Command): string[] {
  switch (command.op) {
    case 'add_atom':
      return command.bond_to ? [command.bond_to.atom] : []
    case 'add_bond':
      return [command.begin, command.end]
    case 'update_atom':
      return [command.atom]
    case 'move_atoms':
    case 'delete_atoms':
      return command.atoms
    case 'update_bond':
      return [command.bond]
    case 'delete_bonds':
      return command.bonds
    case 'rename':
      return []
  }
}

/** The commands if everything they name still exists, otherwise null (the gesture lapses). */
export function stillApplicable(commands: Command[], document: DocumentView): Command[] | null {
  const existing = new Set([...document.atoms.map((atom) => atom.id), ...document.bonds.map((bond) => bond.id)])
  const valid = commands.every((command) =>
    // `$N` names an atom created earlier in the same transaction.
    referencedIds(command).every((id) => id.startsWith('$') || existing.has(id)),
  )
  return valid ? commands : null
}
