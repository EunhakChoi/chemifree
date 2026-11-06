import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { stillApplicable } from '@/editor/commands'
import { validDisplayLabel, type Command, type DocumentView, type FragmentDisplayView } from '@/lib/document-contract'

/** Explicit graph selection and anchor; display text is never parsed as chemistry. */
export function FragmentDisplayPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [text, setText] = useState('')
  const invalidText = text.length > 0 && !validDisplayLabel(text)
  const atoms = document.atoms.filter((atom) => editor.selection.has(atom.id))
  const instance = document.scene?.instances.length === 1 ? document.scene.instances[0].id : null
  function submit(commands: Command[]) {
    void editor.apply((latest) => latest.id === document.id && latest.revision === document.revision ? stillApplicable(commands, latest) : null)
  }
  return <section aria-label="Fragment display" className="flex flex-col gap-2 rounded-lg border p-3">
    <p className="text-xs text-muted-foreground">Select the actual fragment atoms, enter display text, then explicitly choose the boundary anchor. Display text does not define chemistry. The complete retained structure supplies chemical results. One single boundary and a connected tree of neutral default atoms are supported; local stereo and multiple placements are unsupported. Expand before copy, Cut or template export.</p>
    <FieldGroup>
      <Field data-invalid={invalidText}>
        <FieldLabel htmlFor="fragment-display-text">Fragment display text</FieldLabel>
        <Input id="fragment-display-text" value={text} disabled={editor.busy} aria-describedby="fragment-display-help" aria-invalid={invalidText} onChange={(event) => setText(event.target.value)} />
        <FieldDescription id="fragment-display-help">Single line, up to 128 UTF-8 bytes. Uses the document label style.</FieldDescription>
      </Field>
    </FieldGroup>
    <p className="text-xs text-muted-foreground">Selected atoms: {atoms.map((atom) => `${atom.id} (${atom.element})`).join(', ') || 'none'}</p>
    <div className="flex flex-wrap gap-2">
      {instance !== null && atoms.map((atom) => <Button key={atom.id} variant="outline" size="sm" disabled={editor.busy || !validDisplayLabel(text)}
        onClick={() => submit([{ op: 'contract_fragment', instance, anchor: atom.id, members: atoms.map((member) => member.id), text }])}>Contract fragment at {atom.id}</Button>)}
    </div>
    {(document.fragment_displays ?? []).map((item) => <FragmentTextEditor key={`${item.instance}:${item.anchor}:${item.text}`} item={item} busy={editor.busy} submit={submit} />)}
  </section>
}

function FragmentTextEditor({ item, busy, submit }: { item: FragmentDisplayView; busy: boolean; submit: (commands: Command[]) => void }) {
  const [text, setText] = useState(item.text)
  const id = `fragment-text-${item.instance}-${item.anchor}`
  return <form aria-label={`Fragment display at ${item.anchor}`} className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (validDisplayLabel(text)) submit([{ op: 'set_fragment_display_text', instance: item.instance, anchor: item.anchor, text }])
  }}>
    <FieldGroup>
      <Field data-invalid={!validDisplayLabel(text)}>
        <FieldLabel htmlFor={id}>Fragment display at {item.anchor}</FieldLabel>
        <Input id={id} value={text} disabled={busy} aria-describedby={`${id}-help`} aria-invalid={!validDisplayLabel(text)} onChange={(event) => setText(event.target.value)} />
        <FieldDescription id={`${id}-help`}>Display text does not define chemistry. Retained atoms: {item.members.join(', ')}.</FieldDescription>
      </Field>
    </FieldGroup>
    <div className="flex flex-wrap gap-2">
      <Button type="submit" variant="outline" size="sm" disabled={busy || !validDisplayLabel(text) || text === item.text}>Apply fragment display text</Button>
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => submit([{ op: 'expand_fragment', instance: item.instance, anchor: item.anchor }])}>Expand retained fragment at {item.anchor}</Button>
    </div>
  </form>
}
