import { useRef, useState } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { sameInspectionSnapshot, type DocumentView, type QueryInspection, type QueryMatchMode } from '@/lib/document-contract'
import { EMPTY_RULE, queryParameters, type RingCondition, type RuleDraft } from '@/lib/query-parameters'

const MODES: Record<QueryMatchMode, string> = { recorded: 'Recorded bonds', aromatic_target: 'Perceive target aromaticity' }
const RING: Record<RingCondition, string> = { any: 'Any', yes: 'In a ring', no: 'Outside rings' }

/** Query atoms and bonds come from the drawing; optional constraints augment those states. */
export function QueryPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [target, setTarget] = useState('none')
  const [mode, setMode] = useState<QueryMatchMode>('recorded')
  const [selectedAtom, setSelectedAtom] = useState('')
  const [ruleState, setRuleState] = useState<{ documentId: string; rules: Record<string, RuleDraft> }>({ documentId: document.id, rules: {} })
  const [maxMatches, setMaxMatches] = useState('100')
  const [maxStates, setMaxStates] = useState('100000')
  const [busy, setBusy] = useState(false)
  const [answer, setAnswer] = useState<{ key: string; result: QueryInspection } | null>(null)
  const [failure, setFailure] = useState<{ key: string; snapshot: DocumentView; message: string } | null>(null)
  const epoch = useRef(0)
  const documents = editor.list.kind === 'ready' ? editor.list.list.documents : []
  const choices: Record<string, string> = { none: 'Choose a target' }
  for (const item of documents) choices[item.id] = `${item.title} (${item.id})`
  if (target !== 'none' && !(target in choices)) choices[target] = target
  const atomChoices: Record<string, string> = {}
  for (const atom of document.atoms) atomChoices[atom.id] = `${atom.id}: ${atom.element}`
  const atom = selectedAtom in atomChoices ? selectedAtom : document.atoms[0]?.id ?? ''
  const rules = ruleState.documentId === document.id ? ruleState.rules : {}
  const rule = rules[atom] ?? EMPTY_RULE
  const key = JSON.stringify({ mode, target, rules, maxMatches, maxStates, generation: editor.serviceGeneration })
  const current = answer?.key === key && sameInspectionSnapshot(document, answer.result.querySnapshot) ? answer.result : null
  const currentFailure = failure?.key === key && sameInspectionSnapshot(document, failure.snapshot) ? failure.message : null

  function editRule(change: Partial<RuleDraft>) {
    if (!atom) return
    setRuleState((previous) => {
      const existing = previous.documentId === document.id ? previous.rules : {}
      return { documentId: document.id, rules: { ...existing, [atom]: { ...(existing[atom] ?? EMPTY_RULE), ...change } } }
    })
  }

  async function search() {
    const requested = key
    const snapshot = document
    const ticket = ++epoch.current
    const values = queryParameters(document, rules, maxMatches, maxStates)
    setAnswer(null)
    setFailure(null)
    if (!values || target === 'none') {
      setFailure({ key: requested, snapshot, message: 'Choose a target and valid atom conditions and search bounds.' })
      return
    }
    setBusy(true)
    try {
      const result = await editor.matchQuery(document.id, target, mode, values.predicates, values.matches, values.states)
      if (epoch.current === ticket) setAnswer({ key: requested, result })
    } catch (error) {
      if (epoch.current === ticket) setFailure({ key: requested, snapshot, message: error instanceof Error ? error.message : 'Query search failed.' })
    } finally {
      setBusy(false)
    }
  }

  return <section aria-label="Substructure search" className="space-y-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-semibold">Substructure search</h3>
    <Field>
      <FieldLabel htmlFor="query-target">Target drawing</FieldLabel>
      <Select items={choices} value={target} onValueChange={(value) => { if (value) setTarget(value) }}>
        <SelectTrigger id="query-target"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(choices).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
      <FieldDescription>The current drawing is the query. Results describe the captured target.</FieldDescription>
      <Button size="sm" variant="ghost" disabled={editor.list.kind === 'loading'} onClick={() => { epoch.current += 1; setFailure(null); editor.refreshList() }}>Refresh documents</Button>
    </Field>
    <Field>
      <FieldLabel htmlFor="query-mode">Bond interpretation</FieldLabel>
      <Select items={MODES} value={mode} onValueChange={(value) => { if (value && value in MODES) setMode(value as QueryMatchMode) }}>
        <SelectTrigger id="query-mode"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(MODES).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
      <FieldDescription>Perception normalizes supported target rings; query bond predicates remain recorded.</FieldDescription>
    </Field>
    {atom && <>
      <Field>
        <FieldLabel htmlFor="query-atom">Query atom conditions</FieldLabel>
        <Select items={atomChoices} value={atom} onValueChange={(value) => { if (value) setSelectedAtom(value) }}>
          <SelectTrigger id="query-atom"><SelectValue /></SelectTrigger>
          <SelectContent>{Object.entries(atomChoices).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
        </Select>
        <FieldDescription>Conditions are kept separately for each query atom.</FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor="query-ring">Ring</FieldLabel>
        <Select items={RING} value={rule.ring} onValueChange={(value) => { if (value && value in RING) editRule({ ring: value as RingCondition }) }}>
          <SelectTrigger id="query-ring"><SelectValue /></SelectTrigger>
          <SelectContent>{Object.entries(RING).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      <Field><FieldLabel htmlFor="query-min">Minimum neighbors</FieldLabel><Input id="query-min" inputMode="numeric" value={rule.min} onChange={(event) => editRule({ min: event.target.value })} /></Field>
      <Field><FieldLabel htmlFor="query-max">Maximum neighbors</FieldLabel><Input id="query-max" inputMode="numeric" value={rule.max} onChange={(event) => editRule({ max: event.target.value })} /></Field>
      <Field><FieldLabel htmlFor="query-charge">Charge condition</FieldLabel><Input id="query-charge" inputMode="text" value={rule.charge} onChange={(event) => editRule({ charge: event.target.value })} /><FieldDescription>Query atoms only; an element atom must equal its drawn charge.</FieldDescription></Field>
      <Button size="sm" variant="ghost" onClick={() => editRule(EMPTY_RULE)}>Clear this atom's conditions</Button>
      <Button size="sm" variant="ghost" onClick={() => setRuleState({ documentId: document.id, rules: {} })}>Clear all conditions</Button>
    </>}
    <Field><FieldLabel htmlFor="query-matches">Maximum mappings</FieldLabel><Input id="query-matches" inputMode="numeric" value={maxMatches} onChange={(event) => setMaxMatches(event.target.value)} /></Field>
    <Field><FieldLabel htmlFor="query-states">Maximum search work</FieldLabel><Input id="query-states" inputMode="numeric" value={maxStates} onChange={(event) => setMaxStates(event.target.value)} /></Field>
    <Button size="sm" disabled={busy || editor.busy || !atom} onClick={() => void search()}>{busy ? 'Searching…' : 'Search substructure'}</Button>
    {currentFailure && <Alert variant="destructive"><AlertTitle>Could not search</AlertTitle><AlertDescription>{currentFailure}</AlertDescription></Alert>}
    {answer && !current && !currentFailure && <p>The drawing, conditions, target or service changed. Search again.</p>}
    {current && <div className="max-h-64 space-y-2 overflow-auto" aria-live="polite" data-testid="query-result">
      <p>Model: {current.model}. Target snapshot: {current.targetSnapshot.id}, revision {current.targetSnapshot.revision}. Search again to include later edits.</p>
      {current.queryRecovery && <p>A recovery draft was reported for the query and was not included.</p>}
      {current.targetRecovery && <p>A recovery draft was reported for the target and was not included.</p>}
      <p>{current.complete ? `Complete: ${current.mappings.length} mappings.` : `Stopped at the ${current.limit} limit: further mappings may exist.`}</p>
      <p>Search work: {current.states}.</p>
      <ol>{current.mappings.map((mapping, index) => <li key={index}>{index + 1}. {mapping.map((pair) => `${pair.query_atom}→${pair.target_atom}`).join(', ')}</li>)}</ol>
    </div>}
  </section>
}
