/**
 * Document style edits and the units lengths are shown in. Settings are stored in pt whatever
 * the display unit; the unit only changes how lengths are shown and typed. Every edit is built
 * from the latest document when its request runs (see editor/atom-edits), and returns null
 * when it no longer changes anything or would leave the range the service accepts, so quick
 * presses never send a refused or repeated change.
 */
import {
  SETTING_RANGES,
  DEFAULT_SETTINGS,
  type DisplayUnit,
  type DocumentSettings,
  type NumericSetting,
  type PreferencesChange,
  type PreferencesResponse,
  type SettingsEdit,
} from '@/lib/document-contract'
import type { Edit } from '@/editor/atom-edits'

/** How many of each unit one point is, and the decimals a length is shown with. */
export const UNITS: Readonly<Record<DisplayUnit, { label: string; perPoint: number; decimals: number }>> = {
  pt: { label: 'pt', perPoint: 1, decimals: 2 },
  mm: { label: 'mm', perPoint: 25.4 / 72, decimals: 2 },
  cm: { label: 'cm', perPoint: 2.54 / 72, decimals: 3 },
  in: { label: 'in', perPoint: 1 / 72, decimals: 3 },
}

/** What one press of a stepper adds, in pt. */
export const STEPS: Readonly<Record<NumericSetting, number>> = {
  bond_length_pt: 1,
  line_width_pt: 0.1,
  label_size_pt: 1,
}

/** Rounds away the floating-point error of sums and unit conversions. */
const toPrecision = (value: number, decimals: number) => Math.round(value * 10 ** decimals) / 10 ** decimals

/**
 * `pt` in `unit`, rounded to the decimals the unit is shown with. The conversion is first rounded
 * to 9 decimals so that its floating-point error cannot turn an exact half (45 pt is 15.875 mm)
 * into a value just below it.
 */
export function toDisplay(pt: number, unit: DisplayUnit) {
  return toPrecision(toPrecision(pt * UNITS[unit].perPoint, 9), UNITS[unit].decimals)
}

/** A length typed in `unit`, in pt. */
export function fromDisplay(value: number, unit: DisplayUnit) {
  return toPrecision(value / UNITS[unit].perPoint, 4)
}

export function formatLength(pt: number, unit: DisplayUnit) {
  return `${toDisplay(pt, unit)} ${UNITS[unit].label}`
}

export function inRange(field: NumericSetting, value: number) {
  const [low, high] = SETTING_RANGES[field]
  return Number.isFinite(value) && value >= low && value <= high
}

/** What typing a value into a setting's field asks for. */
export type Typed = { kind: 'unchanged' } | { kind: 'invalid' } | { kind: 'set'; pt: number }

/**
 * What `text`, typed into the field of `field` shown in `unit` while the setting is `current`
 * pt, asks for. The value shown (however it is written) is no change. A value within half the
 * last shown digit of a bound is that bound, so the bounds as shown (5 pt is 1.76 mm) can be
 * typed back; anything farther out of range, or not a number, is invalid.
 */
export function typedSetting(field: NumericSetting, text: string, current: number, unit: DisplayUnit): Typed {
  const typed = text.trim() === '' ? Number.NaN : Number(text)
  if (!Number.isFinite(typed)) return { kind: 'invalid' }
  if (typed === toDisplay(current, unit)) return { kind: 'unchanged' }
  const [low, high] = SETTING_RANGES[field]
  const halfDigit = 0.5 / 10 ** UNITS[unit].decimals / UNITS[unit].perPoint
  let pt = fromDisplay(typed, unit)
  if (pt < low && low - pt <= halfDigit) pt = low
  if (pt > high && pt - high <= halfDigit) pt = high
  if (!inRange(field, pt)) return { kind: 'invalid' }
  return pt === current ? { kind: 'unchanged' } : { kind: 'set', pt }
}

const NUMERIC_SETTINGS: readonly NumericSetting[] = ['bond_length_pt', 'line_width_pt', 'label_size_pt']

/** The fields of `edit` that differ from `settings`, or null when any is out of range. */
export function changed(settings: Readonly<DocumentSettings>, edit: SettingsEdit): SettingsEdit | null {
  const result: SettingsEdit = {}
  for (const field of NUMERIC_SETTINGS) {
    const value = edit[field]
    if (value === undefined || value === settings[field]) continue
    if (!inRange(field, value)) return null
    result[field] = value
  }
  if (edit.label_font !== undefined && edit.label_font !== settings.label_font) result.label_font = edit.label_font
  if (edit.carbon_labels !== undefined && edit.carbon_labels !== settings.carbon_labels) {
    result.carbon_labels = edit.carbon_labels
  }
  if (edit.display_unit !== undefined && edit.display_unit !== settings.display_unit) {
    result.display_unit = edit.display_unit
  }
  return result
}

/** Sets the fields of `edit` that the latest document does not already have. */
export function setSettings(edit: SettingsEdit): Edit {
  return (latest) => {
    if (!latest.settings) return null
    const fields = changed(latest.settings, edit)
    return fields && Object.keys(fields).length > 0 ? [{ op: 'update_settings', ...fields }] : null
  }
}

/** `settings` with `field` one step up (1) or down (-1), or null past its range. */
function stepped(settings: Readonly<DocumentSettings>, field: NumericSetting, direction: 1 | -1): SettingsEdit | null {
  const value = toPrecision(settings[field] + direction * STEPS[field], 4)
  if (!inRange(field, value)) return null
  const edit: SettingsEdit = {}
  edit[field] = value
  return edit
}

/** Steps a numeric setting of the latest document by one step up (1) or down (-1). */
export function stepSetting(field: NumericSetting, direction: 1 | -1): Edit {
  return (latest) => {
    const edit = latest.settings && stepped(latest.settings, field, direction)
    return edit ? [{ op: 'update_settings', ...edit }] : null
  }
}

/**
 * The preferences `latest` becomes with the new-document settings `edit` names, storing only
 * what differs from the built-in defaults; null when nothing changes or a value is out of range.
 */
export function preferenceChange(latest: PreferencesResponse, edit: SettingsEdit): PreferencesChange | null {
  const fields = changed(latest.new_document_settings, edit)
  if (!fields || Object.keys(fields).length === 0) return null
  return { ui: latest.ui, new_documents: differencesFromDefaults({ ...latest.new_document_settings, ...fields }) }
}

/** The preferences `latest` becomes with a new-document setting one step up or down. */
export function preferenceStep(latest: PreferencesResponse, field: NumericSetting, direction: 1 | -1) {
  const edit = stepped(latest.new_document_settings, field, direction)
  return edit && preferenceChange(latest, edit)
}

/** The fields of `settings` that differ from the built-in defaults, as preferences store them. */
export function differencesFromDefaults(settings: Readonly<DocumentSettings>): SettingsEdit {
  return changed(DEFAULT_SETTINGS, settings) ?? {}
}
