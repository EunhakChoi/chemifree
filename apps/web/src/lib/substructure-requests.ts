/**
 * Which substructure searches are running and which may still store their outcome. A search
 * belongs to the document it was started for, so one document's search that does not end never
 * keeps another document's from starting, and its late outcome is still stored under its own
 * document. The functions do not change what they are given.
 */

import type { DocumentSummary, DocumentView } from './document-contract'

/** The searches started: a counter for tokens and the latest token of each document. */
export interface Searches {
  next: number
  latest: Map<string, number>
}

export function createSearches(): Searches {
  return { next: 0, latest: new Map() }
}

/** Starts a search for a document: its token, which supersedes the document's earlier one. */
export function beginSearch(searches: Searches, documentId: string): number {
  searches.next += 1
  searches.latest.set(documentId, searches.next)
  return searches.next
}

/** Whether a search still has the right to store its outcome. */
export function isLatest(searches: Searches, documentId: string, token: number): boolean {
  return searches.latest.get(documentId) === token
}

/** Ends interest in every search that is running: none of them stores an outcome. */
export function leaveAll(searches: Searches): void {
  searches.latest.clear()
}

export type Running = ReadonlyMap<string, number>

/** `running` with the document's search marked as running under `token`. */
export function startRunning(running: Running, documentId: string, token: number): Running {
  return new Map(running).set(documentId, token)
}

/** `running` without the document's search, unless a later search has taken its place. */
export function finishRunning(running: Running, documentId: string, token: number): Running {
  if (running.get(documentId) !== token) return running
  const next = new Map(running)
  next.delete(documentId)
  return next
}

export function isRunning(running: Running, documentId: string): boolean {
  return running.has(documentId)
}

/**
 * Whether the other drawing of a result has changed since it was searched, judged by the service's
 * document list: its revision or atom and bond counts are not the searched snapshot's. `searched` is
 * the other drawing's snapshot in the result (`null` when the drawing was searched in itself, which
 * the drawing's own snapshot covers). A drawing the list does not hold is not judged: a title filter
 * or a deletion would look the same, and searching again says which. The list is only as new as its
 * last refresh; pass it only when it was loaded after the search, or an older list reads as a change.
 */
export function otherDrawingChanged(searched: DocumentView | null, summaries: readonly DocumentSummary[]): boolean {
  if (searched === null) return false
  const now = summaries.find((summary) => summary.id === searched.id)
  return now !== undefined && (now.revision !== searched.revision ||
    now.atom_count !== searched.atoms.length || now.bond_count !== searched.bonds.length)
}
