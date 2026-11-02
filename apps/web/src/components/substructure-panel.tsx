import { useEffect, useMemo, useRef, useState } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import {
  SubstructureRefusal,
  sameConversionSnapshot,
  type DocumentView,
  type SubstructureMapping,
  type SubstructureSearch,
} from '@/lib/document-contract'
import {
  beginSearch,
  createSearches,
  finishRunning,
  isLatest,
  isRunning,
  leaveAll,
  startRunning,
  type Running,
} from '@/lib/substructure-requests'

/** What this drawing is in a search. */
type Role = 'query' | 'target'

/** The other drawing chosen in the form: none yet, this drawing itself, or a document ID. */
const NONE = ''
const SELF = 'self'

const FIRST_SHOWN = 20

const ROLES: Record<Role, string> = {
  query: 'query (search it in another drawing)',
  target: 'target (search another drawing in it)',
}

/** What an outcome was asked about; it is shown only while this still holds. */
interface Asked {
  role: Role
  /** The ID of the other drawing; this drawing's own ID when it was searched in itself. */
  other: string
  generation: number
  /** This drawing as the service searched it, or as the page showed it if the request failed. */
  searched: DocumentView
}

type Outcome =
  | (Asked & { kind: 'found'; search: SubstructureSearch })
  | (Asked & { kind: 'refused'; message: string; code: string; query: DocumentView; target: DocumentView })
  | (Asked & { kind: 'failed'; message: string })

function pairsText(pairs: { query: string; target: string }[]) {
  return pairs.map((pair) => `${pair.query}→${pair.target}`).join(', ')
}

/**
 * Substructure search of this drawing in another, or of another in this one, with every atom and
 * bond mapping. A result describes the two snapshots the service searched; it stays shown while
 * this drawing's graph and the service are the ones it was made for, and says so when they are not.
 */
export function SubstructurePanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [role, setRole] = useState<Role>('query')
  const [other, setOther] = useState(NONE)
  // The searches that are running, by document: one document's search that does not end never
  // stops another document from being searched.
  const [running, setRunning] = useState<Running>(new Map())
  // The latest outcome per document: switching documents neither shows nor drops another's.
  const [outcomes, setOutcomes] = useState<ReadonlyMap<string, Outcome>>(new Map())
  const [shownCount, setShownCount] = useState(FIRST_SHOWN)
  // The form is about the drawing it was filled in for: another drawing starts it again, so a
  // choice made for one document is never read as a choice of the next.
  const [formFor, setFormFor] = useState(document.id)
  if (formFor !== document.id) {
    setFormFor(document.id)
    setOther(NONE)
    setShownCount(FIRST_SHOWN)
  }
  const searches = useRef(createSearches())
  // Leaving ends interest in every search that is still running: no outcome is stored.
  useEffect(() => {
    const held = searches.current
    return () => leaveAll(held)
  }, [])

  const documents = editor.list.kind === 'ready' ? editor.list.list.documents : []
  const generation = editor.serviceGeneration
  const outcome = outcomes.get(document.id) ?? null
  // Compared once per change of the drawing or the outcome, not on every render.
  const stillThis = useMemo(() => outcome !== null && sameConversionSnapshot(document, outcome.searched), [document, outcome])
  const drawingChanged = outcome !== null && !stillThis
  const serviceChanged = outcome !== null && outcome.generation !== generation
  const current = outcome !== null && !drawingChanged && !serviceChanged ? outcome : null
  const searching = isRunning(running, document.id)
  const busy = searching || editor.busy

  async function search() {
    if (other === NONE) return
    const otherId = other === SELF ? document.id : other
    const asked = { role, other: otherId, generation, searched: document }
    const documentId = document.id
    const token = beginSearch(searches.current, documentId)
    const store = (next: Outcome) => {
      if (!isLatest(searches.current, documentId, token)) return
      setShownCount(FIRST_SHOWN)
      setOutcomes((all) => new Map(all).set(documentId, next))
    }
    setRunning((current) => startRunning(current, documentId, token))
    try {
      const found = await (role === 'query'
        ? editor.searchSubstructure(document.id, otherId)
        : editor.searchSubstructure(otherId, document.id))
      const searched = role === 'query' ? found.query : found.target
      if (sameConversionSnapshot(document, searched)) {
        store({ ...asked, searched, kind: 'found', search: found })
      } else {
        store({ ...asked, kind: 'failed', message: 'The service searched a different version of this drawing. Search again.' })
      }
    } catch (failure) {
      if (failure instanceof SubstructureRefusal) {
        const searched = role === 'query' ? failure.query : failure.target
        store(sameConversionSnapshot(document, searched)
          ? { ...asked, searched, kind: 'refused', message: failure.message, code: failure.code, query: failure.query, target: failure.target }
          : { ...asked, kind: 'failed', message: 'The service refused a different version of this drawing. Search again.' })
      } else {
        store({ ...asked, kind: 'failed', message: failure instanceof Error ? failure.message : 'The search failed.' })
      }
    } finally {
      setRunning((current) => finishRunning(current, documentId, token))
    }
  }

  function selectInDrawing(mapping: SubstructureMapping, found: Outcome & { kind: 'found' }) {
    const side = found.role === 'query' ? 'query' : 'target'
    editor.setSelection(new Set([...mapping.atoms, ...mapping.bonds].map((pair) => pair[side])))
  }

  const choices: Record<string, string> = { [NONE]: 'Choose a drawing', [SELF]: 'This drawing' }
  for (const item of documents) if (item.id !== document.id) choices[item.id] = `${item.title} (${item.id})`
  if (!(other in choices)) choices[other] = other

  return <section aria-label="Substructure search" className="space-y-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-semibold">Substructure search</h3>
    <p className="leading-relaxed text-muted-foreground">
      Finds every way the query drawing lies in the target drawing. Query atoms and bonds are matched; a drawing the
      aromaticity model cannot read is refused, not searched.
    </p>
    <Field>
      <FieldLabel htmlFor="substructure-role">This drawing is the</FieldLabel>
      <Select items={ROLES} value={role} disabled={busy} onValueChange={(value) => { if (value === 'query' || value === 'target') setRole(value) }}>
        <SelectTrigger id="substructure-role"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(ROLES).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
    </Field>
    <Field>
      <FieldLabel htmlFor="substructure-other">Other drawing</FieldLabel>
      <Select items={choices} value={other} disabled={busy} onValueChange={(value) => { if (value !== null) setOther(value) }}>
        <SelectTrigger id="substructure-other"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(choices).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
      <FieldDescription>The other drawing is searched as the service holds it when you search.</FieldDescription>
      <Button size="sm" variant="ghost" disabled={editor.list.kind === 'loading'} onClick={() => editor.refreshList()}>
        Refresh documents
      </Button>
    </Field>
    <Button size="sm" disabled={busy || other === NONE} onClick={() => void search()}>
      {searching ? 'Searching…' : 'Search'}
    </Button>
    {outcome !== null && (drawingChanged || serviceChanged) && <p role="status" data-testid="substructure-stale">
      {drawingChanged ? 'This drawing changed since the last search.' : 'The service restarted since the last search.'} Search again.
    </p>}
    {current !== null && <p className="text-muted-foreground" data-testid="substructure-asked">
      Asked with this drawing as the {current.role}; the other drawing is {current.other === document.id ? 'this drawing itself' : current.other}.
    </p>}
    {current?.kind === 'failed' && <Alert variant="destructive" data-testid="substructure-error">
      <AlertTitle>The search did not run</AlertTitle>
      <AlertDescription>{current.message}</AlertDescription>
    </Alert>}
    {current?.kind === 'refused' && <Alert variant="destructive" data-testid="substructure-error">
      <AlertTitle>No answer: {current.code.replaceAll('_', ' ')}</AlertTitle>
      <AlertDescription>
        {current.message} Query {current.query.id} revision {current.query.revision}; target {current.target.id} revision {current.target.revision}.
      </AlertDescription>
    </Alert>}
    {current?.kind === 'found' && <div className="max-h-96 space-y-2 overflow-auto" aria-live="polite" data-testid="substructure-result">
      <p>
        {current.search.mappings.length === 0
          ? 'The query does not occur in the target: a complete search found no mapping.'
          : `${current.search.mappings.length} mapping(s) found.`}
      </p>
      <p>
        Query {current.search.query.id} revision {current.search.query.revision}; target {current.search.target.id} revision{' '}
        {current.search.target.revision}. Search again to include later edits to the other drawing.
      </p>
      <p className="text-muted-foreground">
        {current.search.model}; aromaticity {current.search.aromaticityModel}; {current.search.steps} step(s) of at most{' '}
        {current.search.limits.search_steps}.
      </p>
      <ol className="space-y-2">
        {current.search.mappings.slice(0, shownCount).map((mapping, index) => <li key={index} data-testid="substructure-mapping" className="space-y-1">
          <p>Mapping {index + 1}</p>
          <p>Atoms (query→target): {pairsText(mapping.atoms)}</p>
          {mapping.bonds.length > 0 && <p>Bonds (query→target): {pairsText(mapping.bonds)}</p>}
          <Button size="sm" variant="outline" disabled={busy} onClick={() => selectInDrawing(mapping, current)}>
            Select mapping {index + 1} in this drawing
          </Button>
        </li>)}
      </ol>
      {current.search.mappings.length > shownCount && <Button size="sm" variant="ghost" onClick={() => setShownCount(current.search.mappings.length)}>
        Show all {current.search.mappings.length} mappings
      </Button>}
    </div>}
  </section>
}
