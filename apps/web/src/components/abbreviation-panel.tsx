import { useState } from 'react'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { stillApplicable } from '@/editor/commands'
import { validDisplayLabel, type AbbreviationView, type Command, type DocumentView } from '@/lib/document-contract'

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
        {(document.abbreviations ?? []).map((item) => <DisplayLabelEditor key={`${item.instance}:${item.anchor}:${item.display_label}`} item={item} busy={editor.busy} submit={submit} />)}
      </div>
    </section>
  )
}

/** This form edits display metadata, never an atom or a chemical abbreviation definition. */
function DisplayLabelEditor({ item, busy, submit }: {
  item: AbbreviationView
  busy: boolean
  submit: (commands: Command[]) => void
}) {
  const [text, setText] = useState(item.display_label)
  const id = `display-label-${item.instance}-${item.anchor}`
  return <form aria-label={`Display label at ${item.anchor}`} className="flex w-full flex-col gap-2"
    onSubmit={(event) => {
      event.preventDefault()
      if (validDisplayLabel(text)) submit([{ op: 'update_abbreviation_label', instance: item.instance, anchor: item.anchor, text }])
    }}>
    <label htmlFor={id} className="text-sm">Display label at {item.anchor}</label>
    <Input id={id} value={text} disabled={busy} aria-describedby={`${id}-help`} aria-invalid={!validDisplayLabel(text)} onChange={(event) => setText(event.target.value)} />
    <p id={`${id}-help`} className="text-xs text-muted-foreground">Text does not define chemistry; underlying ethyl graph is retained. Single line, up to 128 UTF-8 bytes. Uses the document label style.</p>
    <div className="flex flex-wrap gap-2">
      <Button type="submit" variant="outline" size="sm" disabled={busy || !validDisplayLabel(text) || text === item.display_label}>Apply display label</Button>
      <Button type="button" variant="outline" size="sm" disabled={busy || item.display_label === 'Et'}
        onClick={() => submit([{ op: 'update_abbreviation_label', instance: item.instance, anchor: item.anchor, text: null }])}>Reset display label to Et</Button>
      <Button type="button" variant="outline" size="sm" disabled={busy}
        onClick={() => submit([{ op: 'expand_ethyl', instance: item.instance, anchor: item.anchor }])}>Expand underlying ethyl at {item.anchor}</Button>
    </div>
  </form>
}
