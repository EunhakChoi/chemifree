import { useId } from 'react'

import { ComputeStopNotice } from '@/components/compute-stop-notice'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import type { LayoutController } from '@/hooks/use-layout-preview'
import { layoutMovedCount, type LayoutApplied, type LayoutPreview } from '@/lib/document-contract'
import { layoutRefusalSentence } from '@/lib/layout-refusals'

/**
 * The chain and simple ring layout of the selection in the active placement. The preview changes
 * nothing; Apply is one undoable edit of the previewed proposal. The state lives in the controller.
 */
export function LayoutPanel({ layout, busy }: { layout: LayoutController; busy: boolean }) {
  const switchId = useId()
  const { view } = layout
  return <section aria-label="Chain and ring layout" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
    <h3 className="text-sm font-medium">Chain and ring layout</h3>
    <p className="text-xs text-muted-foreground">Proposes positions for the selected chains and isolated rings of 3 to 12 atoms in the active placement. The preview changes nothing; Apply is one undoable edit.</p>
    {layout.blocked !== null && <p data-testid="layout-hint" className="text-xs text-muted-foreground">{layout.blocked}</p>}
    <div className="flex items-center gap-2">
      <Switch id={switchId} size="sm" checked={layout.scopeSelection} onCheckedChange={layout.setScopeSelection} />
      <Label htmlFor={switchId} className="text-xs">Move only the selected atoms</Label>
    </div>
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={layout.pending || busy || layout.blocked !== null} onClick={() => void layout.preview()}>
        {layout.pending ? 'Previewing…' : 'Preview layout'}
      </Button>
      <Button size="sm" variant="outline" disabled={view.kind !== 'preview' || busy || layout.pending} onClick={() => layout.apply()}>Apply layout</Button>
      {(view.kind === 'preview' || view.kind === 'stale') && <Button size="sm" variant="outline" onClick={() => layout.clear()}>Clear preview</Button>}
    </div>
    <LayoutViewBlock layout={layout} busy={busy} />
  </section>
}

/** The stop, the refusal, the failure, the result or the applied note of the current view; nothing while idle or pending. */
function LayoutViewBlock({ layout, busy }: { layout: LayoutController; busy: boolean }) {
  const { view } = layout
  switch (view.kind) {
    case 'idle':
    case 'pending':
      return null
    case 'stopped':
      return <ComputeStopNotice stop={view.stop} onRetry={() => void layout.preview()} disabled={layout.pending || busy} />
    case 'refused':
      return <div role="alert" data-testid="layout-refusal" data-layout-reason={view.refusal.reason} className="text-xs">
        <p>{layoutRefusalSentence(view.refusal.reason, view.refusal.limit) ?? view.refusal.message}</p>
        {view.refusal.atoms.length > 0 && <p>Atoms: {view.refusal.atoms.join(', ')}</p>}
      </div>
    case 'failed':
      return <p role="alert" className="text-xs">{view.message}</p>
    case 'stale':
      return <p data-testid="layout-stale" className="text-xs">The drawing, selection or service changed. Preview again.</p>
    case 'preview':
      return <PreviewResult preview={view.preview} />
    case 'applied':
      return <AppliedResult applied={view.applied} />
    default: {
      const unhandled: never = view
      throw new Error(`Unknown layout view: ${JSON.stringify(unhandled)}`)
    }
  }
}

function PreviewResult({ preview }: { preview: LayoutPreview }) {
  const n = preview.atoms.length
  const c = preview.componentCount
  const r = preview.ringCount
  const m = layoutMovedCount(preview)
  const overlap = preview.overlap
  return <div aria-live="polite" data-testid="layout-result" className="flex max-h-64 flex-col gap-2 overflow-auto text-xs">
    <p>Model: {preview.model}</p>
    <p data-testid="layout-counts">{`${n} ${n === 1 ? 'atom' : 'atoms'} in ${c} ${c === 1 ? 'component' : 'components'}, ${r} ${r === 1 ? 'ring' : 'rings'}; ${m} ${m === 1 ? 'atom moves' : 'atoms move'}.`}</p>
    <p>Placement {preview.request.instance}; {preview.request.scope === 'selection' ? 'selected atoms only' : 'connected structures'}.</p>
    {overlap && <p data-testid="layout-overlap">{`${overlapStatement(overlap.count)}; overlaps are not resolved: ${overlap.pairs.map(([a, b]) => `${a}–${b}`).join(', ')}`}</p>}
    {preview.noBonds && <p>No bonds lie between the laid-out atoms.</p>}
    <p className="text-muted-foreground">{preview.notImplemented}</p>
  </div>
}

function AppliedResult({ applied }: { applied: LayoutApplied }) {
  const overlap = applied.overlap
  return <div aria-live="polite" data-testid="layout-applied" data-layout-unchanged={String(applied.unchanged)} className="flex flex-col gap-1 text-xs">
    {applied.unchanged
      ? <p>Every layout atom is already at its proposed position; nothing was changed.</p>
      : <p>Layout applied as one edit. Undo restores the previous positions.</p>}
    {overlap && <p data-testid="layout-overlap">{`${overlapStatement(overlap.count)}; overlaps are not resolved.`}</p>}
  </div>
}

/** The count sentence of an overlap warning, shared by the preview and the applied note. */
function overlapStatement(count: number): string {
  return `${count} atom ${count === 1 ? 'pair is' : 'pairs are'} closer than half a bond length`
}
