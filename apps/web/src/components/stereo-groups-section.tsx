import { useId, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  GROUP_ACTIONS,
  GROUPS_NOT_EXAMINED,
  GROUPS_NOTE,
  groupRows,
  parseGroupNumber,
  planGroupRequest,
  reseedDraft,
  type GroupRequest,
  type GroupRow,
} from '@/editor/stereo-groups'
import type { DocumentView } from '@/lib/document-contract'

interface StereoGroupsSectionProps {
  document: DocumentView
  /** Ids selected on the canvas (atoms count; bonds and drawn objects are ignored). */
  selected: readonly string[]
  /** Sends one edit; resolves when it has been answered, whether or not it was refused. */
  onRequest: (request: GroupRequest) => Promise<void>
  /** Selects one atom, as a click on the canvas would. */
  onSelectAtom: (atom: string) => void
  groupLabels: boolean
  onGroupLabelsChange: (on: boolean) => void
  busy: boolean
}

/**
 * Enhanced stereo groups of the stored centers: a list built only from the snapshot, and the
 * one-click edits on the selected atoms. A control that cannot apply is disabled and says why in
 * text; the reason comes from the same function that builds the click (`planGroupRequest`).
 * Nothing is stored by typing: a number or action is sent only on a click.
 */
export function StereoGroupsSection({ document, selected, onRequest, onSelectAtom, groupLabels, onGroupLabelsChange, busy }: StereoGroupsSectionProps) {
  const switchId = useId()
  const numberId = useId()
  const [numberText, setNumberText] = useState('')
  const rows = groupRows(document)
  const parsed = parseGroupNumber(numberText)

  return (
    <section aria-label="Enhanced stereo groups" className="flex flex-col gap-2 border-t pt-2 text-xs" data-testid="stereo-groups">
      <h4 className="text-xs font-medium">Enhanced stereo groups</h4>
      <p className="text-muted-foreground" data-testid="groups-note">{GROUPS_NOTE}</p>
      <p className="text-muted-foreground" data-testid="groups-not-examined">{GROUPS_NOT_EXAMINED}</p>
      <div className="flex items-center gap-2">
        <Switch id={switchId} size="sm" checked={groupLabels} onCheckedChange={onGroupLabelsChange} />
        <Label htmlFor={switchId} className="text-xs">Group labels</Label>
      </div>
      <div className="flex flex-col gap-1">
        <Label htmlFor={numberId} className="text-xs">Number for a new or split group (empty for none)</Label>
        <Input
          id={numberId}
          className="h-7 w-32 text-xs md:text-xs"
          inputMode="numeric"
          autoComplete="off"
          value={numberText}
          aria-invalid={parsed.kind === 'invalid'}
          onChange={(event) => setNumberText(event.target.value)}
        />
        {parsed.kind === 'invalid' && <p role="alert" data-testid="group-number-error">{parsed.why}</p>}
      </div>
      <ul className="flex flex-col gap-1.5" data-testid="group-actions">
        {GROUP_ACTIONS.map(({ action, label }) => {
          const plan = planGroupRequest(document, selected, { action, numberText })
          return (
            <li key={action} className="flex flex-col gap-0.5" data-action={action}>
              <Button
                size="sm"
                variant="outline"
                className="self-start"
                disabled={busy || !plan.ok}
                onClick={() => void onRequest({ action, numberText })}
              >
                {label}
              </Button>
              {!plan.ok && <p className="text-muted-foreground" data-testid="group-why">{plan.why}</p>}
            </li>
          )
        })}
      </ul>
      {rows.length === 0 ? (
        <p data-testid="groups-empty">No groups. A center in no group is simply ungrouped; that states nothing.</p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="group-list">
          {rows.map((row) => (
            <GroupItem key={row.id} row={row} document={document} selected={selected} onRequest={onRequest} onSelectAtom={onSelectAtom} busy={busy} />
          ))}
        </ul>
      )}
    </section>
  )
}

function GroupItem({
  row, document, selected, onRequest, onSelectAtom, busy,
}: { row: GroupRow; document: DocumentView; selected: readonly string[]; onRequest: StereoGroupsSectionProps['onRequest']; onSelectAtom: (atom: string) => void; busy: boolean }) {
  const inputId = useId()
  const [text, setText] = useState(row.numberText)
  // The stored number this field last followed. When it changes (undo, redo) the field follows it without a remount,
  // unless the person is typing in it (see `reseedDraft`).
  const [followed, setFollowed] = useState(row.numberText)
  const [focused, setFocused] = useState(false)
  if (followed !== row.numberText) {
    setFollowed(row.numberText)
    setText(reseedDraft(text, followed, row.numberText, focused))
  }
  const set = planGroupRequest(document, selected, { action: 'set_number', group: row.id, numberText: text })
  return (
    <li className="flex flex-col gap-1 border-t pt-2" data-testid="stereo-group" data-group={row.id} data-kind={row.kind}>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="font-medium">{row.id}</span>
        <Badge variant="outline" data-testid="group-label">{row.label}</Badge>
        <Badge variant="secondary">{row.kindWord}</Badge>
        <span data-testid="group-count">{row.count} {row.count === 1 ? 'member' : 'members'}</span>
      </div>
      <p className="text-muted-foreground">{row.kindWord}: {row.meaning}</p>
      <p data-testid="group-number">{row.number}</p>
      <ul className="flex flex-wrap gap-1" aria-label={`Members of ${row.id}`}>
        {row.members.map((member) => (
          <li key={member}>
            <Button size="xs" variant="outline" data-testid="group-member" aria-label={`Select ${member}`} onClick={() => onSelectAtom(member)}>{member}</Button>
          </li>
        ))}
      </ul>
      {row.kind !== 'absolute' && (
        <div className="flex flex-wrap items-end gap-2">
          <div className="flex flex-col gap-1">
            <Label htmlFor={inputId} className="text-xs">Number of {row.id}</Label>
            <Input id={inputId} className="h-7 w-28 text-xs md:text-xs" inputMode="numeric" autoComplete="off" value={text} onChange={(event) => setText(event.target.value)} onFocus={() => setFocused(true)} onBlur={() => setFocused(false)} aria-invalid={parseGroupNumber(text).kind === 'invalid'} />
          </div>
          <Button
            size="sm"
            variant="outline"
            disabled={busy || !set.ok}
            aria-label={`Set the number of ${row.id}`}
            onClick={() => {
              // After a refusal the field returns to the stored number: the page keeps no edit the service did not take.
              void onRequest({ action: 'set_number', group: row.id, numberText: text }).then(() => setText(row.numberText))
            }}
          >
            Set number
          </Button>
        </div>
      )}
      {row.kind !== 'absolute' && !set.ok && <p className="text-muted-foreground" data-testid="group-number-why">{set.why}</p>}
      <Button size="sm" variant="outline" className="self-start" disabled={busy} aria-label={`Delete group ${row.id}`} onClick={() => void onRequest({ action: 'delete_group', group: row.id })}>
        Delete group
      </Button>
    </li>
  )
}
