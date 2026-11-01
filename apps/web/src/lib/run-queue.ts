import { ApiError } from '@/lib/document-contract'

export function asApiError(error: unknown): ApiError {
  return error instanceof ApiError ? error : new ApiError('unexpected', 'The request failed unexpectedly.', 0)
}

export interface RunQueueHooks {
  /** Called with +1 when a task is queued and -1 when it has settled. */
  onPending: (delta: 1 | -1) => void
  /** Called with the page error to show, or null to clear it. */
  onError: (error: ApiError | null) => void
}

/**
 * Runs tasks one at a time behind earlier ones and keeps the page error. A success clears an
 * error the user had already seen when the task was queued, never one from a request that
 * failed while it waited.
 */
export class RunQueue {
  private tail: Promise<void> = Promise.resolve()
  private failures = 0
  private readonly hooks: RunQueueHooks

  constructor(hooks: RunQueueHooks) {
    this.hooks = hooks
  }

  /** A failure becomes the page error; the returned promise always resolves. */
  run(task: () => Promise<void>): Promise<void> {
    this.hooks.onPending(1)
    const failuresSeen = this.failures
    const next = this.tail
      .then(task)
      .then(
        () => {
          if (this.failures === failuresSeen) this.hooks.onError(null)
        },
        (failure: unknown) => {
          this.failures += 1
          this.hooks.onError(asApiError(failure))
        },
      )
      .finally(() => this.hooks.onPending(-1))
    this.tail = next
    return next
  }

  /**
   * Like `run`, but a failure is rethrown to the caller, whose panel shows it, and is not the
   * page error. A success clears a page error exactly as `run` does.
   */
  runOrThrow<T>(task: () => Promise<T>): Promise<T> {
    this.hooks.onPending(1)
    const failuresSeen = this.failures
    const outcome = this.tail.then(task)
    const settled = outcome
      .then(
        () => {
          if (this.failures === failuresSeen) this.hooks.onError(null)
        },
        () => {
          // The caller reports this failure; the queue only has to keep going.
        },
      )
      .finally(() => this.hooks.onPending(-1))
    this.tail = settled
    // Resolve or reject only once the bookkeeping is done, so the caller sees a settled queue.
    return settled.then(() => outcome)
  }

  /** Records a failure found outside a task as the page error. */
  report(failure: ApiError): void {
    this.failures += 1
    this.hooks.onError(failure)
  }
}
