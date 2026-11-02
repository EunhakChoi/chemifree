/**
 * Which substructure searches are running and which may still store their outcome. A search
 * belongs to the document it was started for, so one document's search that does not end never
 * keeps another document's from starting, and its late outcome is still stored under its own
 * document. The functions do not change what they are given.
 */

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
