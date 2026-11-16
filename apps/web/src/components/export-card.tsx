import { useId, useState, type ChangeEvent } from 'react'

import { CtExportFlow } from '@/components/ctfile-panel'
import { InterchangeExportFlow } from '@/components/interchange-export-panel'
import { SmilesExportFlow } from '@/components/smiles-panel'
import { SvgExportFlow } from '@/components/svg-export-panel'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { DEFAULT_CTAB, type CtabChoice } from '@/lib/ct-conversion'
import type { DocumentView, SmilesStyle } from '@/lib/document-contract'
import { EXPORT_FORMATS, parseExportFormat, type ExportFormat } from '@/lib/export-formats'

/**
 * The one Export card of an open drawing (P48 S5). The chosen format mounts that format's existing
 * export flow, keyed by the format, so changing the format drops a prepared result, an offer and late
 * replies. The MOL version and SMILES form live as long as the card, which the workspace keys by
 * document; nothing is stored in the browser or in preferences.
 */
export function ExportCard({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [format, setFormat] = useState<ExportFormat | null>(null)
  const [ctab, setCtab] = useState<CtabChoice>(DEFAULT_CTAB)
  const [smilesStyle, setSmilesStyle] = useState<SmilesStyle>('recorded')
  const titleId = useId()

  function chooseFormat(event: ChangeEvent<HTMLSelectElement>) {
    const next = parseExportFormat(event.currentTarget.value)
    if (next !== null) setFormat(next)
  }

  function renderFlow() {
    switch (format) {
      case null:
        return <p role="status" data-testid="export-choose">Choose a format. Nothing is sent until you press a preview or write button.</p>
      case 'smiles':
        return <SmilesExportFlow key="smiles" document={document} editor={editor} initialStyle={smilesStyle} onStyleChange={setSmilesStyle} />
      case 'mol':
      case 'sdf':
        return <CtExportFlow key={format} document={document} editor={editor} format={format} initialChoice={ctab} onChoiceChange={setCtab} />
      case 'cdxml':
      case 'cdx':
        return <InterchangeExportFlow key={format} document={document} editor={editor} format={format} />
      case 'svg':
        return <SvgExportFlow key="svg" document={document} editor={editor} />
    }
  }

  return (
    <section aria-labelledby={titleId} data-testid="export-card" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
      <h3 id={titleId} className="text-sm font-medium">Export</h3>
      <p className="text-muted-foreground">Exports this drawing as a whole, never only the selection. A file or text is offered only after any listed losses are accepted.</p>
      <Field>
        <FieldLabel htmlFor="export-format">Export format</FieldLabel>
        <select id="export-format" value={format ?? ''} disabled={editor.busy} className="h-8 rounded-lg border border-input bg-transparent px-2 text-xs" onChange={chooseFormat}>
          <option value="" disabled>Choose a format</option>
          {EXPORT_FORMATS.map(({ id, label }) => <option key={id} value={id}>{label}</option>)}
        </select>
        <FieldDescription>Changing the format drops a prepared result and a download still being checked; the MOL version and SMILES form are kept while this drawing stays open and are not saved in it.</FieldDescription>
      </Field>
      {renderFlow()}
    </section>
  )
}
