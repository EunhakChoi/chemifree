import { LossList } from '@/components/loss-list'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { useConsentedExport } from '@/hooks/use-consented-export'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView, SvgExport } from '@/lib/document-contract'

/** Offers the whole drawing as an SVG picture under the shared export loss consent. The picture is not an interchange file. */
export function SvgExportPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const {
    exported,
    current: currentExport,
    offer,
    currentOffer,
    currentUnacceptable,
    pending,
    error,
    busy,
    preview,
    download,
  } = useConsentedExport<'svg', SvgExport>({
    document,
    editor,
    run: (id, _kind, accepted) => editor.exportSvg(id, accepted),
    blob: (result) => new Blob([result.content], { type: result.media_type }),
    fileName: () => 'chemifree.svg',
  })

  return <aside aria-label="SVG drawing export" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SVG drawing export</h3>
    <p className="leading-relaxed text-muted-foreground" data-testid="svg-export-note">Writes the whole drawing as one SVG picture of its atoms, bonds, labels, scene graphics, PNG images, and captions whose characters are all in a bounded character set. The SVG is a picture, and its graph is not machine-readable, so a drawing with atoms always reports chemical_graph. It is not an interchange file and cannot be imported back: SVG import is refused. Fonts are not embedded and glyphs are not converted to outlines, so the viewer's installed font chooses the glyph shapes. Radical and attachment marks, stereo configurations and groups, label styling, abbreviation displays, caption underlines and captions with other characters are left out or drawn differently and must be accepted first.</p>
    <Button size="sm" variant="outline" disabled={busy} onClick={() => void preview('svg')}>Preview SVG drawing</Button>
    {pending > 0 && <p role="status" className="flex items-center gap-2"><Spinner />Checking export…</p>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {currentUnacceptable && <div role="group" aria-label="Losses that need consent" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>These losses ({currentUnacceptable.features.join(', ')}) need consent, but this reply has no consent digest to accept. No download is available.</p>
      <LossList losses={currentUnacceptable.losses} />
    </div>}
    {offer && !currentOffer && <p>The drawing or service changed. Preview the export again.</p>}
    {currentOffer && <div role="group" aria-label="Losses to accept before writing" className="flex max-h-64 flex-col gap-2 overflow-auto" data-testid="svg-export-offer">
      {currentOffer.changed && <p>The losses changed since you accepted them. Review them again.</p>}
      <p>Writing this SVG needs these losses accepted: {currentOffer.features.join(', ')}.</p>
      <LossList losses={currentOffer.losses} />
      <Button size="sm" disabled={busy} onClick={() => void preview('svg', currentOffer.digest, currentOffer.changed)}>
        Accept these losses and preview SVG
      </Button>
    </div>}
    {exported && !currentExport && <p>The drawing or service changed. Preview the export again.</p>}
    {currentExport && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite" data-testid="svg-export-written">
      <p>Prepared an SVG drawing of {currentExport.result.view_box.width} × {currentExport.result.view_box.height} pt (model {currentExport.result.model}, label advance table {currentExport.result.font_table}).</p>
      {currentExport.result.view_box.width === 0 && currentExport.result.view_box.height === 0 && <p>Nothing is drawn; the SVG is 0 by 0 pt.</p>}
      <LossList losses={currentExport.result.losses} />
      <Button size="sm" disabled={busy} onClick={() => void download(currentExport)}>Download SVG</Button>
    </div>}
  </aside>
}
