import { useId, useMemo, useRef } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  ACTION_LABELS,
  AGREEMENT_LABELS,
  actionName,
  agreementRows,
  listedDiagnostics,
  NOT_JUDGED,
  NOTHING_TO_COMPARE,
  rowAction,
  rowSentence,
  updatableRows,
  viewSentence,
  type AgreementRow,
  type AgreementView,
  type Resolution,
} from '@/editor/stereo-agreement'
import { cn } from '@/lib/utils'

interface StereoPanelProps {
  view: AgreementView
  /** Whether a change to the drawing is compared on its own. */
  automatic: boolean
  onAutomaticChange: (automatic: boolean) => void
  onCheckNow: () => void
  /** Sends the one-click edit for a row (or all the disagreements). */
  onResolve: (resolution: Resolution) => void
  busy: boolean
}

const itemName = (row: AgreementRow) => `${row.item.kind === 'atom' ? 'Center' : 'Double bond'} ${row.item.id}`

/**
 * How each stored configuration compares with the drawing of each placement, as the service
 * reports it for exactly the drawing on screen, and the one-click edits that resolve a row. It
 * never stores anything by itself, and it judges nothing stereogenic.
 */
export function StereoPanel({ view, automatic, onAutomaticChange, onCheckNow, onResolve, busy }: StereoPanelProps) {
  const heading = useRef<HTMLHeadingElement>(null)
  const switchId = useId()
  const inspection = view.kind === 'ready' ? view.inspection : null
  const rows = useMemo(() => (inspection ? agreementRows(inspection) : []), [inspection])
  const all = inspection ? updatableRows(inspection, rows) : []
  const findings = inspection ? listedDiagnostics(inspection.diagnostics) : []

  // The row that was clicked goes away while the drawing is compared again; keep the focus here.
  const resolve = (resolution: Resolution) => {
    heading.current?.focus()
    onResolve(resolution)
  }

  return (
    <section aria-label="Stereo" aria-busy={view.kind === 'checking'} className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
      <h3 ref={heading} tabIndex={-1} className="text-sm font-medium outline-none">Stereo</h3>
      <p className="text-xs text-muted-foreground">{NOT_JUDGED}</p>
      {view.kind === 'none' ? (
        <p className="text-xs" data-testid="stereo-panel-state">{NOTHING_TO_COMPARE}</p>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <div className="flex items-center gap-2">
              <Switch id={switchId} size="sm" checked={automatic} onCheckedChange={onAutomaticChange} />
              <Label htmlFor={switchId} className="text-xs">Check automatically</Label>
            </div>
            {view.kind === 'failed' ? (
              <Button size="sm" variant="outline" onClick={onCheckNow}>Check again</Button>
            ) : (
              !automatic && <Button size="sm" variant="outline" disabled={view.kind === 'checking'} onClick={onCheckNow}>Check now</Button>
            )}
          </div>
          <p className="text-xs" data-testid="stereo-panel-state">{viewSentence(view)}</p>
          {view.kind === 'failed' && <p role="alert" className="text-xs">{view.message}</p>}
        </>
      )}
      {inspection && (
        <>
          {all.length > 0 && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              aria-label={`Update all from drawing (${all.length} stored configurations)`}
              onClick={() => resolve({ kind: 'update_all', items: all.map((row) => row.item), instance: all[0].instance })}
            >
              Update all from drawing
            </Button>
          )}
          <ul className="flex max-h-72 flex-col gap-2 overflow-auto" data-testid="stereo-rows">
            {rows.map((row) => {
              const action = rowAction(row.agreement)
              return (
                <li
                  key={row.key}
                  data-testid="stereo-row"
                  data-agreement={row.agreement}
                  data-item={row.item.id}
                  data-instance={row.instance}
                  className="flex flex-col gap-1 border-t pt-2 text-xs"
                >
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-medium">{itemName(row)}</span>
                    <Badge variant="outline">{row.instance}</Badge>
                    <Badge variant={row.agreement === 'disagrees' ? 'destructive' : row.agreement === 'agrees' ? 'secondary' : 'outline'}>
                      {row.agreement === 'disagrees' && <span aria-hidden="true">≠</span>}
                      {AGREEMENT_LABELS[row.agreement]}
                    </Badge>
                  </div>
                  <p>{rowSentence(row)}</p>
                  {action && (
                    <Button
                      size="sm"
                      variant="outline"
                      className="self-start"
                      disabled={busy}
                      aria-label={actionName(row, action)}
                      onClick={() => resolve({ kind: action, item: row.item, instance: row.instance })}
                    >
                      {ACTION_LABELS[action]}
                    </Button>
                  )}
                </li>
              )
            })}
          </ul>
          {rows.some((row) => row.agreement === 'disagrees') && (
            <p className="text-xs text-muted-foreground">To keep a stored configuration instead, change the drawn marks or move atoms until the drawing agrees.</p>
          )}
          {findings.length > 0 && (
            <Collapsible>
              <CollapsibleTrigger className={cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'px-1')}>
                Details ({findings.length} advisory finding{findings.length === 1 ? '' : 's'})
              </CollapsibleTrigger>
              <CollapsibleContent>
                <ul className="flex flex-col gap-1.5 pt-1 text-xs" data-testid="stereo-findings">
                  {findings.map((finding, index) => (
                    <li key={`${finding.code}:${index}`}>
                      <span className="font-mono">{finding.code}</span> ({finding.severity}): {finding.message}
                    </li>
                  ))}
                </ul>
              </CollapsibleContent>
            </Collapsible>
          )}
        </>
      )}
    </section>
  )
}
