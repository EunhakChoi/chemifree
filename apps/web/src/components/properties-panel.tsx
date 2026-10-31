import { useState } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { selectionSnapshot } from '@/editor/scene-selection'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import {
  samePropertySnapshot,
  type DocumentView,
  type OmittedIsotopeAbundance,
  type PropertiesInspection,
  type PropertiesScope,
  type PropertyKind,
  type SelectionView,
} from '@/lib/document-contract'

const SCOPES: Record<PropertiesScope, string> = { molecule: 'Whole molecule', selection: 'Selection', components: 'Each component' }
const KINDS: Record<PropertyKind, string> = { formula: 'Formula', masses: 'Formula and mass', distribution: 'Isotope distribution' }

function key(scope: PropertiesScope, property: PropertyKind, selection: SelectionView | null, source: string, generation: number) {
  return JSON.stringify({ scope, property, selection, source, generation })
}

function isotopeList(values: OmittedIsotopeAbundance[]) {
  return values.map(({ element, isotope }) => `${isotope}${element}`).join(', ')
}

/** Read-only results stay tied to the selected content and the service that produced them. */
export function PropertiesPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [scope, setScope] = useState<PropertiesScope>('molecule')
  const [property, setProperty] = useState<PropertyKind>('formula')
  const [source, setSource] = useState('')
  const [busy, setBusy] = useState(false)
  const [answer, setAnswer] = useState<{ key: string; result: PropertiesInspection } | null>(null)
  const [failure, setFailure] = useState<{ key: string; snapshot: DocumentView; message: string } | null>(null)
  const currentSelection = scope === 'selection' ? selectionSnapshot(editor.selection, document) : null
  const currentSource = property === 'formula' ? '' : source.trim()
  const currentKey = key(scope, property, currentSelection, currentSource, editor.serviceGeneration)
  const current = answer?.key === currentKey && samePropertySnapshot(document, answer.result.snapshot) ? answer.result : null
  const currentFailure = failure?.key === currentKey && samePropertySnapshot(document, failure.snapshot) ? failure.message : null

  async function calculate() {
    const selection = scope === 'selection' ? selectionSnapshot(editor.selectionAtPress(), document) : null
    const sourceId = property === 'formula' ? '' : source.trim()
    const requested = key(scope, property, selection, sourceId, editor.serviceGeneration)
    setBusy(true)
    setFailure(null)
    setAnswer(null)
    try {
      const result = await editor.inspectProperties(document.id, scope, property, selection, sourceId || null)
      setAnswer({ key: requested, result })
    } catch (error) {
      setFailure({ key: requested, snapshot: document, message: error instanceof Error ? error.message : 'Property calculation failed.' })
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="space-y-3 rounded-lg border p-3" aria-label="Molecular properties">
      <h3 className="text-sm font-semibold">Molecular properties</h3>
      <Field>
        <FieldLabel htmlFor="property-scope">Scope</FieldLabel>
        <Select items={SCOPES} value={scope} onValueChange={(value) => { if (value && value in SCOPES) setScope(value as PropertiesScope) }}>
          <SelectTrigger id="property-scope"><SelectValue /></SelectTrigger>
          <SelectContent>{Object.entries(SCOPES).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      <Field>
        <FieldLabel htmlFor="property-kind">Result</FieldLabel>
        <Select items={KINDS} value={property} onValueChange={(value) => { if (value && value in KINDS) setProperty(value as PropertyKind) }}>
          <SelectTrigger id="property-kind"><SelectValue /></SelectTrigger>
          <SelectContent>{Object.entries(KINDS).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      {property !== 'formula' && (
        <Field>
          <FieldLabel htmlFor="property-source">Data registration ID</FieldLabel>
          <Input id="property-source" value={source} onChange={(event) => setSource(event.currentTarget.value)} placeholder="Automatic if only one" />
          <FieldDescription>Mass results need an available external dataset.</FieldDescription>
        </Field>
      )}
      <Button size="sm" onClick={() => void calculate()} disabled={busy || editor.busy}>
        {busy ? 'Calculating…' : 'Calculate'}
      </Button>
      {currentFailure && <Alert variant="destructive"><AlertTitle>Could not calculate</AlertTitle><AlertDescription>{currentFailure}</AlertDescription></Alert>}
      {answer && !current && !currentFailure && <p className="text-xs text-muted-foreground">The drawing, selection or request changed. Calculate again.</p>}
      {current && (
        <div className="space-y-2 text-xs" data-testid="properties-result">
          {current.components.map((component, index) => (
            <div key={component.atoms.join(',')} className="space-y-1 rounded border p-2">
              {scope === 'components' && <p>Component {index + 1}</p>}
              <p><strong>Formula:</strong> {component.formula}</p>
              <p>Formal charge: {component.formal_charge}</p>
              {component.mass && <>
                <p>Average: {component.mass.average_display} u</p>
                <p>Average basis: {component.mass.average_bases.map((row) => `${row.element} (${row.basis})`).join(', ') || 'explicit isotopes'}</p>
                <p>Exact-profile: {component.mass.exact_display} u</p>
                <p>Exact isotopes: {component.mass.exact_selections.map((row) => `${row.isotope}${row.element} (${row.basis})`).join(', ') || 'natural-weight fallback'}</p>
                {component.mass.exact_natural_weight_fallbacks.length > 0 && <p>Natural-weight fallback: {component.mass.exact_natural_weight_fallbacks.join(', ')}</p>}
                {component.mass.average_omitted_abundances.length > 0 && <p>Average omits unknown abundances: {isotopeList(component.mass.average_omitted_abundances)}</p>}
                {component.mass.exact_omitted_abundances.length > 0 && <p>Exact selection omits unknown abundances: {isotopeList(component.mass.exact_omitted_abundances)}</p>}
              </>}
              {component.isotope_distribution && <>
                <p>Isotope peaks ({component.isotope_distribution.length})</p>
                <ol className="max-h-40 overflow-y-auto font-mono">
                  {component.isotope_distribution.map((peak, peakIndex) => <li key={peakIndex}>{peak.mass_display} u · {peak.probability}</li>)}
                </ol>
                {component.omitted_abundances.length > 0 && <p>Unknown abundances omitted: {isotopeList(component.omitted_abundances)}</p>}
                <p>Reported sums: {component.abundance_sums.map((row) => `${row.element} ${row.input_sum}`).join(', ') || 'not applicable'}</p>
              </>}
            </div>
          ))}
          {current.warnings.map((warning, index) => <p key={index} className="text-amber-700 dark:text-amber-300">{warning}</p>)}
          <details>
            <summary>Method and data</summary>
            <p>Model: {current.model}; valence: {current.valence_model}{current.property !== 'formula' && `; display: ${current.display_decimals} decimal places`}.</p>
            <p>Arithmetic: {current.arithmetic}</p>
            {current.data && <>
              <p>Data: {current.data.dataset_id} · {current.data.dataset_version} · {current.data.calculation_profile}</p>
              <p>Registration: {current.data.registration_id}, generation {current.data.generation}, profile {current.data.profile_id}, manifest {current.data.manifest_sha256}</p>
              {current.data.source_schema_id && <p>Source schema: {current.data.source_schema_id}</p>}
              <p>Charge correction: {current.data.charge_correction_u_per_e} u per elementary charge</p>
              <p>{current.data.charge_convention}</p>
              <p>{current.data.abundance_policy}</p>
              <p>{current.data.uncertainty}</p>
            </>}
          </details>
        </div>
      )}
    </section>
  )
}
