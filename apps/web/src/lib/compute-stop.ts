// No imports: document-contract.ts imports this reader, so importing that module back would form a cycle.

/** A computation that ended without a result because its time limit or a cancel stopped it. */
export type ComputeStop = { kind: 'time_limit'; limitMs: number } | { kind: 'cancelled' }

/** What a stop reply must carry for one operation. */
export interface StopExpectation {
  /** The models the operation reports; a stop names one of them. */
  models: readonly string[]
  /**
   * `count`: the reply names the drawing's revision. `count_or_null`: the reply may name it as
   * null, as the aromaticity conversion's stop does today.
   */
  revision: 'count' | 'count_or_null'
}

/** What every stop shows under its title: nothing changed, and no partial result. */
export const STOP_DETAIL = 'The computation was stopped before it finished. No result is shown, and the drawing was not changed.'

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isEmptyArray(value: unknown): boolean {
  return Array.isArray(value) && value.length === 0
}

function isRevision(value: unknown, revision: StopExpectation['revision']): boolean {
  if (value === null) return revision === 'count_or_null'
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
}

/**
 * Reads a reply as a stop. Returns null when the reply is not a stop (a result, a refusal, or any
 * other error), so the caller reads it as before. Returns `'malformed'` when the reply carries a
 * stop code but not the stop contract, so the caller refuses it instead of showing a guess.
 * Otherwise returns the stop, with the limit it reached.
 */
export function readComputeStop(reply: unknown, expect: StopExpectation): ComputeStop | 'malformed' | null {
  if (!isRecord(reply) || reply.outcome !== 'error') return null
  const diagnostics: unknown = reply.diagnostics
  if (!Array.isArray(diagnostics) || diagnostics.length === 0) return null
  const first: unknown = diagnostics[0]
  if (!isRecord(first) || (first.code !== 'time_limit' && first.code !== 'cancelled')) return null

  if (
    diagnostics.length !== 1 ||
    first.severity !== 'error' ||
    typeof first.message !== 'string' ||
    first.path !== null ||
    reply.result !== null ||
    !isEmptyArray(reply.loss_report) ||
    typeof reply.model_id !== 'string' ||
    !expect.models.includes(reply.model_id) ||
    !isRevision(reply.document_revision, expect.revision) ||
    !isRecord(first.details)
  ) {
    return 'malformed'
  }
  if (first.code === 'cancelled') return { kind: 'cancelled' }
  const limitMs = first.details.limit_ms
  if (typeof limitMs !== 'number' || !Number.isSafeInteger(limitMs) || limitMs < 0) return 'malformed'
  return { kind: 'time_limit', limitMs }
}

/** The title of a stop: the limit it reached, or that the computation was cancelled. */
export function stopTitle(stop: ComputeStop): string {
  switch (stop.kind) {
    case 'time_limit':
      return `Stopped at the time limit (${stop.limitMs} ms)`
    case 'cancelled':
      return 'Cancelled'
    default: {
      const unreachable: never = stop
      throw new Error(`Unknown compute stop: ${JSON.stringify(unreachable)}`)
    }
  }
}
