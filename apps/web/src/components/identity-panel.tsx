import { useState } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { sameInspectionSnapshot, type CanonicalIdentity, type DocumentView, type IdentityInspection, type IdentityMode } from '@/lib/document-contract'

const MODES: Record<IdentityMode, string> = { recorded: 'Recorded graph', chemical: 'Bounded chemical model' }

function requestKey(mode: IdentityMode, other: string, generation: number) {
  return JSON.stringify({ mode, other, generation })
}

function IdentityDetails({ label, identity }: { label: string; identity: CanonicalIdentity }) {
  return <div className="space-y-1">
    <p className="font-medium">{label}</p>
    <p>Canonical numbers: {identity.numbering.map((id, index) => `${index + 1}→${id}`).join(', ') || '(empty)'}</p>
    <p>Symmetry groups: {identity.symmetry.map((group) => group.join(', ')).join(' | ') || '(none)'}</p>
    {identity.folded_hydrogens.length > 0 && <p>Folded natural H: {identity.folded_hydrogens.map((row) => `${row.hydrogen}→${row.attached_to}`).join(', ')}</p>}
  </div>
}

/** Comparison results describe the captured pair, not later edits to another document. */
export function IdentityPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [mode, setMode] = useState<IdentityMode>('recorded')
  const [other, setOther] = useState('none')
  const [busy, setBusy] = useState(false)
  const [answer, setAnswer] = useState<{ key: string; result: IdentityInspection } | null>(null)
  const [failure, setFailure] = useState<{ key: string; snapshot: DocumentView; message: string } | null>(null)
  const documents = editor.list.kind === 'ready' ? editor.list.list.documents : []
  const choices: Record<string, string> = { none: 'No comparison' }
  for (const item of documents) choices[item.id] = `${item.title} (${item.id})`
  if (other !== 'none' && !(other in choices)) choices[other] = other
  const key = requestKey(mode, other, editor.serviceGeneration)
  const current = answer?.key === key && sameInspectionSnapshot(document, answer.result.snapshot) ? answer.result : null
  const currentFailure = failure?.key === key && sameInspectionSnapshot(document, failure.snapshot) ? failure.message : null

  async function inspect() {
    const requested = requestKey(mode, other, editor.serviceGeneration)
    const snapshot = document
    setBusy(true)
    setAnswer(null)
    setFailure(null)
    try {
      const result = await editor.inspectIdentity(document.id, mode, other === 'none' ? null : other)
      setAnswer({ key: requested, result })
    } catch (error) {
      setFailure({ key: requested, snapshot, message: error instanceof Error ? error.message : 'Identity inspection failed.' })
    } finally {
      setBusy(false)
    }
  }

  return <section aria-label="Graph identity" className="space-y-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-semibold">Graph identity</h3>
    <Field>
      <FieldLabel htmlFor="identity-mode">Comparison model</FieldLabel>
      <Select items={MODES} value={mode} onValueChange={(value) => { if (value && value in MODES) setMode(value as IdentityMode) }}>
        <SelectTrigger id="identity-mode"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(MODES).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
      <FieldDescription>Recorded compares drawn states; the chemical model normalizes only supported states.</FieldDescription>
    </Field>
    <Field>
      <FieldLabel htmlFor="identity-other">Compare with</FieldLabel>
      <Select items={choices} value={other} onValueChange={(value) => { if (value) setOther(value) }}>
        <SelectTrigger id="identity-other"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(choices).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
      <FieldDescription>The comparison uses the other document as captured when inspected.</FieldDescription>
      <Button size="sm" variant="ghost" disabled={editor.list.kind === 'loading'} onClick={editor.refreshList}>Refresh documents</Button>
    </Field>
    <Button size="sm" disabled={busy || editor.busy} onClick={() => void inspect()}>{busy ? 'Inspecting…' : 'Inspect identity'}</Button>
    {currentFailure && <Alert variant="destructive"><AlertTitle>Could not inspect identity</AlertTitle><AlertDescription>{currentFailure}</AlertDescription></Alert>}
    {answer && !current && !currentFailure && <p>The drawing, model, comparison or service changed. Inspect again.</p>}
    {current && <div className="max-h-64 space-y-2 overflow-auto" aria-live="polite" data-testid="identity-result">
      <p>Model: {current.model}. Symmetric atoms may exchange canonical numbers.</p>
      {current.comparison && <p>{current.comparison.equivalent ? 'Equivalent' : 'Different'} under this model. Comparison snapshot: {current.comparison.snapshot.id}, revision {current.comparison.snapshot.revision}. Inspect again to include later edits.</p>}
      {current.comparison?.recovery && <p>A recovery draft was reported for the comparison document and was not included.</p>}
      <IdentityDetails label="Current drawing" identity={current.identity} />
      {current.comparison && <IdentityDetails label="Comparison snapshot" identity={current.comparison.identity} />}
    </div>}
  </section>
}
