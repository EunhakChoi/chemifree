import { Button } from '@/components/ui/button'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { stillApplicable } from '@/editor/commands'
import type { Command, DocumentView } from '@/lib/document-contract'

/** Explicit Et display controls. Scientific eligibility is checked by the shared domain. */
export function AbbreviationPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const atoms = document.atoms.filter((atom) => editor.selection.has(atom.id))
  const placements = document.scene?.instances ?? []
  const instance = placements.length === 1 ? placements[0].id : null
  function submit(commands: Command[]) {
    const id = document.id
    void editor.apply((latest) => latest.id === id ? stillApplicable(commands, latest) : null)
  }
  return (
    <section aria-label="Ethyl abbreviation" className="flex flex-col gap-2 rounded-lg border p-3">
      <p className="text-xs text-muted-foreground">Select the two neutral default carbons of a terminal ethyl group, then choose its boundary anchor. Et changes the display only. Expand before native copy, Cut or template export. Multiple placements and local stereo are unsupported.</p>
      <div className="flex flex-wrap gap-2">
        {atoms.length === 2 && instance !== null && atoms.map((atom) => {
          const terminal = atoms.find((other) => other.id !== atom.id)
          return terminal && <Button key={atom.id} variant="outline" size="sm" disabled={editor.busy}
            onClick={() => submit([{ op: 'collapse_ethyl', instance, anchor: atom.id, terminal: terminal.id }])}>Collapse Et at {atom.id}</Button>
        })}
        {(document.abbreviations ?? []).map((item) => <Button key={`${item.instance}:${item.anchor}`} variant="outline" size="sm" disabled={editor.busy}
          onClick={() => submit([{ op: 'expand_ethyl', instance: item.instance, anchor: item.anchor }])}>Expand Et at {item.anchor}</Button>)}
      </div>
    </section>
  )
}
