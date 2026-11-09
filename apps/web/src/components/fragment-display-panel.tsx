import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { stillApplicable } from '@/editor/commands'
import { fragmentDisplayRuns, validDisplayLabel, validFragmentDisplayRuns, type Command, type DocumentView, type FragmentDisplayRun, type FragmentDisplayView } from '@/lib/document-contract'

// Copies only the run fields this client knows: a reply may carry others, and requests refuse unknown fields.
function knownFragmentRun(run: FragmentDisplayRun): FragmentDisplayRun {
  return { text: run.text, script: run.script }
}

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
    {(document.fragment_displays ?? []).map((item) => <FragmentTextEditor key={`${document.id}:${item.instance}:${item.anchor}:${JSON.stringify(fragmentDisplayRuns(item))}`} item={item} busy={editor.busy} submit={submit} />)}
  </section>
}

function FragmentTextEditor({ item, busy, submit }: { item: FragmentDisplayView; busy: boolean; submit: (commands: Command[]) => void }) {
  const [text, setText] = useState(item.text)
  const id = `fragment-text-${item.instance}-${item.anchor}`
  const baseline = fragmentDisplayRuns(item)
  const unchanged = text === item.text && baseline.length === 1 && baseline[0].script === 'baseline'
  return <div className="flex flex-col gap-2"><form aria-label={`Fragment display at ${item.anchor}`} className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (validDisplayLabel(text)) submit([{ op: 'set_fragment_display_text', instance: item.instance, anchor: item.anchor, text }])
  }}>
    <FieldGroup>
      <Field data-invalid={!validDisplayLabel(text)}>
        <FieldLabel htmlFor={id}>Fragment display at {item.anchor}</FieldLabel>
        <Input id={id} value={text} disabled={busy} aria-describedby={`${id}-help`} aria-invalid={!validDisplayLabel(text)} onChange={(event) => setText(event.target.value)} />
        <FieldDescription id={`${id}-help`}>Plain text resets all runs to baseline. Display text does not define chemistry. Retained atoms: {item.members.join(', ')}.</FieldDescription>
      </Field>
    </FieldGroup>
    <div className="flex flex-wrap gap-2">
      <Button type="submit" variant="outline" size="sm" disabled={busy || !validDisplayLabel(text) || unchanged}>Apply fragment display text</Button>
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => submit([{ op: 'expand_fragment', instance: item.instance, anchor: item.anchor }])}>Expand retained fragment at {item.anchor}</Button>
    </div>
  </form>
    <FragmentRunEditor item={item} busy={busy} submit={submit} />
  </div>
}

/** Whole Unicode runs use native input and IME behavior, matching caption editing. */
function FragmentRunEditor({ item, busy, submit }: { item: FragmentDisplayView; busy: boolean; submit: (commands: Command[]) => void }) {
  const id = useId()
  const original = fragmentDisplayRuns(item)
  const [runs, setRuns] = useState<FragmentDisplayRun[]>(() => original.map((run) => knownFragmentRun(run)))
  const [composing, setComposing] = useState(false)
  const valid = validFragmentDisplayRuns(runs)
  const unchanged = JSON.stringify(runs) === JSON.stringify(original.map((run) => knownFragmentRun(run)))
  return <form aria-label={`Fragment runs at ${item.anchor}`} className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (busy || composing || !valid || unchanged) return
    // Freeze the submitted value before the editor request queue can run.
    const frozen = runs.map((run) => knownFragmentRun(run))
    submit([{ op: 'set_fragment_display_runs', instance: item.instance, anchor: item.anchor, runs: frozen }])
  }}>
    <p className="text-xs text-muted-foreground">Script changes appearance only. All runs use the drawing label style. Each run must be nonempty; together they must contain nonblank, single-line text within 128 UTF-8 bytes. Run boundaries are preserved and may split shaping; keep combining marks and emoji sequences together.</p>
    <FieldGroup>
      {runs.map((run, index) => <FieldGroup key={index}>
        <Field data-invalid={run.text.length === 0}>
          <FieldLabel htmlFor={`${id}-text-${index}`}>Fragment run {index + 1} text</FieldLabel>
          <Input id={`${id}-text-${index}`} value={run.text} disabled={busy} aria-invalid={run.text.length === 0}
            onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
            onChange={(event) => setRuns((current) => current.map((entry, at) => at === index ? { ...entry, text: event.target.value } : entry))} />
        </Field>
        <Field>
          <FieldLabel id={`${id}-script-${index}`}>Fragment run {index + 1} script</FieldLabel>
          <ToggleGroup variant="outline" size="sm" disabled={busy || composing} aria-labelledby={`${id}-script-${index}`} value={[run.script]} onValueChange={(values) => {
            const script = values[0]
            if (script === 'baseline' || script === 'subscript' || script === 'superscript') setRuns((current) => current.map((entry, at) => at === index ? { ...entry, script } : entry))
          }}>
            <ToggleGroupItem value="baseline">Baseline</ToggleGroupItem>
            <ToggleGroupItem value="subscript">Subscript</ToggleGroupItem>
            <ToggleGroupItem value="superscript">Superscript</ToggleGroupItem>
          </ToggleGroup>
        </Field>
        <Button type="button" variant="outline" size="sm" disabled={busy || composing || runs.length === 1} onClick={() => setRuns((current) => current.filter((_, at) => at !== index))}>Remove fragment run {index + 1}</Button>
      </FieldGroup>)}
    </FieldGroup>
    <div className="flex flex-wrap gap-2">
      <Button type="button" variant="outline" size="sm" disabled={busy || composing} onClick={() => setRuns((current) => [...current, { text: '', script: 'baseline' }])}>Add fragment run</Button>
      <Button type="submit" variant="outline" size="sm" disabled={busy || composing || !valid || unchanged}>Apply fragment runs</Button>
    </div>
  </form>
}
