import { useRef, useState, type KeyboardEvent } from 'react'
import { MinusIcon, PaletteIcon, PlusIcon, SettingsIcon } from 'lucide-react'

import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { Spinner } from '@/components/ui/spinner'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import {
  UNITS,
  formatLength,
  preferenceChange,
  preferenceStep,
  setSettings,
  stepSetting,
  toDisplay,
  typedSetting,
} from '@/editor/settings'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { PreferencesEditor } from '@/hooks/use-preferences'
import {
  CARBON_LABELS,
  DISPLAY_UNITS,
  LABEL_FONTS,
  SETTING_RANGES,
  THEMES,
  type CarbonLabels,
  type ComputationSetting,
  type DisplayUnit,
  type DocumentSettings,
  type DocumentView,
  type LabelFont,
  type NumericSetting,
  type SettingsEdit,
  type Theme,
} from '@/lib/document-contract'

const FONT_NAMES: Record<LabelFont, string> = { 'sans-serif': 'Sans-serif', serif: 'Serif', monospace: 'Monospace' }
const CARBON_NAMES: Record<CarbonLabels, string> = {
  default: 'Only carbons that need one',
  all: 'Every carbon',
}
const UNIT_NAMES: Record<DisplayUnit, string> = { pt: 'Points (pt)', mm: 'Millimetres (mm)', cm: 'Centimetres (cm)', in: 'Inches (in)' }
const THEME_NAMES: Record<Theme, string> = { system: 'Follow the system', light: 'Light', dark: 'Dark' }
const MASS_DECIMAL_NAMES = Object.fromEntries(
  Array.from({ length: 9 }, (_, value) => [String(value), value > 5 ? `${value} (set by another client)` : String(value)]),
)

function blurOnEnter(event: KeyboardEvent<HTMLInputElement>) {
  if (event.key === 'Enter') event.currentTarget.blur()
}

/**
 * A setting stored in pt, typed and shown in `unit`, with buttons that step it by its step in
 * pt. A typed value is committed on blur or Enter; one out of range is refused here (the
 * service would refuse it too) and nothing is sent.
 */
function NumberSetting({
  id,
  label,
  field,
  value,
  unit,
  onSet,
  onStep,
}: {
  id: string
  label: string
  field: NumericSetting
  value: number
  unit: DisplayUnit
  onSet: (edit: SettingsEdit) => Promise<void>
  onStep: (field: NumericSetting, direction: 1 | -1) => void
}) {
  const shown = String(toDisplay(value, unit))
  const [invalid, setInvalid] = useState(false)
  // Keep a draft and the unit it was typed in while earlier requests update the document.
  const [draft, setDraft] = useState<{ text: string; unit: DisplayUnit; initial: number } | null>(null)
  const editing = useRef<typeof draft>(null)
  const [low, high] = SETTING_RANGES[field]
  const name = label.toLowerCase()

  function commit() {
    const pending = editing.current
    if (!pending) return
    editing.current = null
    const typed = typedSetting(field, pending.text, pending.initial, pending.unit)
    setInvalid(typed.kind === 'invalid')
    if (typed.kind === 'invalid') {
      setDraft(null)
      return
    }
    const edit: SettingsEdit = {}
    // An earlier queued write may change the value before this edit runs. Preserve the
    // exact shown value and let the queue compare it with the latest settings.
    edit[field] = typed.kind === 'set' ? typed.pt : pending.initial
    const clear = () => setDraft((current) => current === pending ? null : current)
    void onSet(edit).then(clear, clear)
  }

  return (
    <Field data-invalid={invalid || undefined}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <div className="flex items-center gap-2">
        <Button size="icon-sm" variant="outline" aria-label={`Decrease ${name}`} onClick={() => onStep(field, -1)}>
          <MinusIcon aria-hidden="true" />
        </Button>
        <Input
          id={id}
          value={draft?.text ?? shown}
          inputMode="decimal"
          aria-invalid={invalid || undefined}
          className="w-24 font-mono"
          onChange={(event) => {
            const next = { ...(editing.current ?? { unit, initial: value }), text: event.currentTarget.value }
            editing.current = next
            setDraft(next)
          }}
          onKeyDown={blurOnEnter}
          onBlur={commit}
        />
        <span className="text-sm text-muted-foreground">{UNITS[draft?.unit ?? unit].label}</span>
        <Button size="icon-sm" variant="outline" aria-label={`Increase ${name}`} onClick={() => onStep(field, 1)}>
          <PlusIcon aria-hidden="true" />
        </Button>
      </div>
      {invalid && (
        <FieldError>
          {label} must be from {formatLength(low, unit)} to {formatLength(high, unit)}.
        </FieldError>
      )}
    </Field>
  )
}

/** One of a closed set of values, chosen from a select. */
function ChoiceSetting<T extends string>({
  id,
  label,
  value,
  options,
  names,
  onChange,
}: {
  id: string
  label: string
  value: T
  options: readonly T[]
  names: Record<T, string>
  onChange: (value: T) => void
}) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Select
        items={names}
        value={value}
        onValueChange={(next) => {
          const chosen = options.find((option) => option === next)
          if (chosen !== undefined) onChange(chosen)
        }}
      >
        <SelectTrigger id={id} className="w-56">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option} value={option}>
              {names[option]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}

/**
 * Every document setting. Lengths are shown in the settings' display unit; the label size is
 * a font size and stays in pt.
 */
function SettingsFields({
  idPrefix,
  settings,
  onSet,
  onStep,
}: {
  idPrefix: string
  settings: Readonly<DocumentSettings>
  onSet: (edit: SettingsEdit) => Promise<void>
  onStep: (field: NumericSetting, direction: 1 | -1) => void
}) {
  const unit = settings.display_unit
  return (
    <FieldGroup className="gap-3">
      <NumberSetting
        id={`${idPrefix}-bond-length`}
        label="Bond length"
        field="bond_length_pt"
        value={settings.bond_length_pt}
        unit={unit}
        onSet={onSet}
        onStep={onStep}
      />
      <NumberSetting
        id={`${idPrefix}-line-width`}
        label="Line width"
        field="line_width_pt"
        value={settings.line_width_pt}
        unit={unit}
        onSet={onSet}
        onStep={onStep}
      />
      <NumberSetting
        id={`${idPrefix}-label-size`}
        label="Label size"
        field="label_size_pt"
        value={settings.label_size_pt}
        unit="pt"
        onSet={onSet}
        onStep={onStep}
      />
      <ChoiceSetting
        id={`${idPrefix}-label-font`}
        label="Label font"
        value={settings.label_font}
        options={LABEL_FONTS}
        names={FONT_NAMES}
        onChange={(label_font) => void onSet({ label_font })}
      />
      <ChoiceSetting
        id={`${idPrefix}-carbon-labels`}
        label="Carbon labels"
        value={settings.carbon_labels}
        options={CARBON_LABELS}
        names={CARBON_NAMES}
        onChange={(carbon_labels) => void onSet({ carbon_labels })}
      />
      <ChoiceSetting
        id={`${idPrefix}-display-unit`}
        label="Display unit"
        value={settings.display_unit}
        options={DISPLAY_UNITS}
        names={UNIT_NAMES}
        onChange={(display_unit) => void onSet({ display_unit })}
      />
    </FieldGroup>
  )
}

/**
 * The document's style: its own settings, saved and undone with it. Every change is built from
 * the latest document when its request runs, so quick presses step from each other's results.
 * The settings can be reset or taken from another saved document, each as one undoable edit.
 */
export function DocumentStyleButton({
  editor,
  document,
  settings,
}: {
  editor: DocumentEditor
  document: DocumentView
  settings: Readonly<DocumentSettings>
}) {
  const others = editor.list.kind === 'ready' ? editor.list.list.documents.filter((item) => item.id !== document.id) : []
  const names = Object.fromEntries(others.map((item) => [item.id, `${item.title} (${item.id})`]))
  const [source, setSource] = useState<string | null>(null)
  const chosen = others.find((item) => item.id === source)?.id ?? null
  return (
    <Dialog>
      <Tooltip>
        <TooltipTrigger
          render={
            <DialogTrigger render={<Button size="icon-sm" variant="outline" aria-label="Document style" />}>
              <PaletteIcon aria-hidden="true" />
            </DialogTrigger>
          }
        />
        <TooltipContent>Document style</TooltipContent>
      </Tooltip>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Document style</DialogTitle>
          <DialogDescription>
            Saved with “{document.title}” and undone like any edit. Lengths are stored in pt and shown in{' '}
            {UNITS[settings.display_unit].label}. Preferences never change it.
          </DialogDescription>
        </DialogHeader>
        <SettingsFields
          idPrefix="style"
          settings={settings}
          onSet={(edit) => editor.apply(setSettings(edit))}
          onStep={(field, direction) => void editor.apply(stepSetting(field, direction))}
        />
        <Separator />
        <FieldSet>
          <FieldLegend variant="label">Reset</FieldLegend>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" onClick={() => void editor.resetSettings('defaults')}>
              Built-in defaults
            </Button>
            <Button size="sm" variant="outline" onClick={() => void editor.resetSettings('preferences')}>
              New-document preferences
            </Button>
          </div>
        </FieldSet>
        <Field>
          <FieldLabel htmlFor="style-import-source">Use the style of</FieldLabel>
          <div className="flex flex-wrap items-center gap-2">
            <Select items={names} value={chosen} onValueChange={(next) => setSource(others.find((item) => item.id === next)?.id ?? null)}>
              <SelectTrigger id="style-import-source" className="w-56" disabled={others.length === 0}>
                <SelectValue placeholder={others.length === 0 ? 'No other saved documents' : 'A saved document'} />
              </SelectTrigger>
              <SelectContent>
                {others.map((item) => (
                  <SelectItem key={item.id} value={item.id}>
                    {names[item.id]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button size="sm" variant="outline" disabled={chosen === null} onClick={() => chosen && void editor.importSettings(chosen)}>
              Apply style
            </Button>
          </div>
          <FieldDescription>Takes every setting of that document as one undoable edit.</FieldDescription>
        </Field>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Done</DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ComputationRow({ id, label, setting }: { id: string; label: string; setting: ComputationSetting }) {
  return (
    <Field data-disabled>
      <FieldLabel htmlFor={id}>
        {label} <Badge variant="outline">Not implemented</Badge>
      </FieldLabel>
      <Input id={id} value="Not implemented" disabled readOnly className="w-56" />
      <FieldDescription>{setting.note}</FieldDescription>
    </Field>
  )
}

/**
 * The service's preferences: the theme, the style new documents start from, and the
 * mass display decimals, and unavailable general computation settings. A new document copies its style once,
 * when it is created; changing preferences never changes a document that exists.
 */
export function PreferencesButton({
  preferences,
  current,
  editor,
}: {
  preferences: PreferencesEditor
  current: DocumentView | null
  editor: DocumentEditor
}) {
  const { state, error } = preferences
  return (
    <Dialog>
      <DialogTrigger render={<Button size="sm" variant="ghost" />}>
        <SettingsIcon data-icon="inline-start" aria-hidden="true" />
        Preferences
      </DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Preferences</DialogTitle>
          <DialogDescription>
            Kept by the local service for its data directory, apart from every document. A new document copies the
            new-document style once, when it is created; changing it never changes a document that exists.
          </DialogDescription>
        </DialogHeader>
        {error && (
          <Alert variant={error.code === 'revision_conflict' ? 'default' : 'destructive'}>
            <AlertTitle>{error.code === 'revision_conflict' ? 'Preferences changed elsewhere' : 'Not saved'}</AlertTitle>
            <AlertDescription>
              {error.code === 'revision_conflict' ? 'Another client changed the preferences first; they are shown as they are now.' : error.message}
            </AlertDescription>
            <AlertAction>
              <Button size="sm" variant="ghost" onClick={preferences.dismissError}>Dismiss</Button>
            </AlertAction>
          </Alert>
        )}
        {state.kind === 'loading' && <Spinner />}
        {state.kind === 'error' && (
          <div className="space-y-2">
            <p className="text-sm text-muted-foreground">Preferences could not be read: {state.message}</p>
            <Button size="sm" variant="outline" onClick={() => void preferences.retry()}>Retry preferences</Button>
          </div>
        )}
        {state.kind === 'ready' && (
          <>
            <ChoiceSetting
              id="preferences-theme"
              label="Theme"
              value={state.preferences.ui.theme}
              options={THEMES}
              names={THEME_NAMES}
              onChange={(theme) =>
                void preferences.set((latest) =>
                  latest.ui.theme === theme ? null : { ui: { theme }, new_documents: latest.new_documents },
                )
              }
            />
            <Separator />
            <FieldSet>
              <FieldLegend>New documents</FieldLegend>
              <FieldDescription>The style a document created from now on starts with.</FieldDescription>
              <SettingsFields
                idPrefix="new-documents"
                settings={state.preferences.new_document_settings}
                onSet={(edit) => preferences.set((latest) => preferenceChange(latest, edit))}
                onStep={(field, direction) => void preferences.set((latest) => preferenceStep(latest, field, direction))}
              />
              <div className="flex flex-wrap gap-2">
                {current?.settings && (
                  <Button size="sm" variant="outline" onClick={() => {
                    const reservation = editor.reserveStyle()
                    void preferences.fromDocument(current.id, reservation.ready).finally(reservation.release)
                  }}>
                    Use the style of “{current.title}”
                  </Button>
                )}
                <Button size="sm" variant="outline" onClick={() => void preferences.reset()}>
                  Reset preferences
                </Button>
              </div>
            </FieldSet>
            <Separator />
            <FieldSet>
              <FieldLegend>Computation</FieldLegend>
              <FieldDescription>
                Settings belong to preferences, apart from documents. Mass display decimals apply to new results;
                they do not change calculation precision or earlier results.
              </FieldDescription>
              {state.preferences.computation.mass_precision && (
                <Field>
                  <FieldLabel htmlFor="preferences-mass-decimals">Mass display decimal places</FieldLabel>
                  <Select
                    items={MASS_DECIMAL_NAMES}
                    value={String(state.preferences.computation.mass_precision.decimals)}
                    onValueChange={(next) => {
                      if (next === null || !/^[0-5]$/.test(next)) return
                      const decimals = Number(next)
                      void preferences.set((latest) => latest.computation.mass_precision?.decimals === decimals ? null : {
                        ui: latest.ui,
                        new_documents: latest.new_documents,
                        computation: { mass_precision: { decimals } },
                      })
                    }}
                  >
                    <SelectTrigger id="preferences-mass-decimals" className="w-56"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {[0, 1, 2, 3, 4, 5].map((value) => <SelectItem key={value} value={String(value)}>{value}</SelectItem>)}
                      {state.preferences.computation.mass_precision.decimals > 5 && (
                        <SelectItem value={String(state.preferences.computation.mass_precision.decimals)}>
                          {state.preferences.computation.mass_precision.decimals} (set by another client)
                        </SelectItem>
                      )}
                    </SelectContent>
                  </Select>
                  <FieldDescription>The editor offers 0–5 places; other clients may set 0–8.</FieldDescription>
                </Field>
              )}
              <ComputationRow id="preferences-nmr" label="NMR prediction" setting={state.preferences.computation.nmr} />
              <ComputationRow
                id="preferences-precision"
                label="Numeric precision"
                setting={state.preferences.computation.numeric_precision}
              />
            </FieldSet>
          </>
        )}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Done</DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
