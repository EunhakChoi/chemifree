import { MAX_CHARGE, MIN_CHARGE } from '@/editor/atom-edits'
import type { DocumentView, QueryAtomPredicate } from '@/lib/document-contract'

export type RingCondition = 'any' | 'yes' | 'no'
/** The text a user typed for one query atom's conditions. */
export interface RuleDraft { ring: RingCondition; min: string; max: string; charge: string }
export const EMPTY_RULE: RuleDraft = { ring: 'any', min: '', max: '', charge: '' }

/** Empty text is `undefined`; text that is not an integer in range is `null`. */
function integer(text: string, minimum: number, maximum: number): number | undefined | null {
  if (!text.trim()) return undefined
  if (!/^-?\d+$/.test(text.trim())) return null
  const value = Number(text)
  return Number.isSafeInteger(value) && value >= minimum && value <= maximum ? value : null
}

/**
 * Validates the search bounds and per-atom conditions against the drawn query. Conditions kept
 * for atoms no longer in the drawing are ignored. Returns null when anything is invalid,
 * including a charge condition that contradicts an element atom's drawn charge.
 */
export function queryParameters(
  document: DocumentView,
  rules: Record<string, RuleDraft>,
  maxMatches: string,
  maxStates: string,
): { predicates: QueryAtomPredicate[]; matches: number; states: number } | null {
  const matches = integer(maxMatches, 1, 1000)
  const states = integer(maxStates, 1, 1_000_000)
  if (matches == null || states == null) return null
  const predicates: QueryAtomPredicate[] = []
  for (const [id, draft] of Object.entries(rules)) {
    const drawn = document.atoms.find((candidate) => candidate.id === id)
    if (!drawn) continue
    const min = integer(draft.min, 0, 2000)
    const max = integer(draft.max, 0, 2000)
    const charge = integer(draft.charge, MIN_CHARGE, MAX_CHARGE)
    if (min === null || max === null || charge === null || (min !== undefined && max !== undefined && min > max)) return null
    if (charge !== undefined && drawn.kind === 'element' && charge !== drawn.charge) return null
    const predicate: QueryAtomPredicate = { atom: id }
    if (draft.ring !== 'any') predicate.ring = draft.ring === 'yes'
    if (min !== undefined) predicate.min_neighbors = min
    if (max !== undefined) predicate.max_neighbors = max
    if (charge !== undefined) predicate.charge = charge
    if (Object.keys(predicate).length > 1) predicates.push(predicate)
  }
  return { predicates, matches, states }
}
