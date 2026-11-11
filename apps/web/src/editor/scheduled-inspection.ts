/**
 * The part the Stereo agreement check and the CIP label request share: when a read-only
 * inspection of the drawing on screen is asked for, which answer may be shown, and what a
 * detached (unmounted) runner does with a request that is still in flight. It is a plain class
 * with an injectable clock so that every ordering runs in a node test; the hook
 * (`hooks/use-scheduled-inspection.ts`) only wires it to React.
 */
import { CheckScheduler, type Timers } from '@/editor/stereo-agreement'
import type { ComputeStop } from '@/lib/compute-stop'
import { ComputeStopped, samePropertySnapshot, type DocumentView } from '@/lib/document-contract'

/** What the runner is told on every render. */
export interface InspectionSource<T> {
  /** The drawing on screen. */
  document: DocumentView
  /** Changes when the service process does: a reply from another process is not used. */
  generation: number
  /** True while a pointer gesture is in progress; no request starts until it ends. */
  dragging: boolean
  /** Whether a change is asked about on its own. */
  active: boolean
  /** Whether a request may run at all (a manual "check now" needs this without `active`). */
  allowed: boolean
  inspect: (documentId: string) => Promise<T>
}

/** The last reply or failure, and the service process it came from. */
export type InspectionOutcome<T> =
  | { kind: 'ok'; generation: number; inspection: T }
  /** `stop` is the stop that ended the request (a time limit or a cancel), or null for any other failure. */
  | { kind: 'failed'; generation: number; snapshot: DocumentView; message: string; stop: ComputeStop | null }

export interface InspectionState<T> {
  outcome: InspectionOutcome<T> | null
  running: boolean
}

/** What was last asked about. */
export interface Asked {
  document: DocumentView
  generation: number
  active: boolean
}

/**
 * Keep an unchanged source, including a pending manual check while automatic asking is off.
 * A changed inactive source stops a pending wait; a changed active source asks again.
 */
export function askDecision(before: Asked | null, next: Asked): 'stop' | 'keep' | 'ask' {
  const repeat = before !== null && before.active === next.active && before.generation === next.generation && samePropertySnapshot(before.document, next.document)
  if (repeat) return 'keep'
  return next.active ? 'ask' : 'stop'
}

export class InspectionRunner<T> {
  private state: InspectionState<T> = { outcome: null, running: false }
  private readonly listeners = new Set<() => void>()
  private scheduler: CheckScheduler | null = null
  private asked: Asked | null = null
  private dragging = false
  /** Identifies the request allowed to report; detaching or switching off retires it. */
  private run = 0
  private source: InspectionSource<T>
  private readonly timers: Timers
  private readonly dropOnStop: boolean

  /**
   * With `dropOnStop`, turning the asking off clears the last outcome and discards a request still
   * in flight: what was shown before is not shown again when asking resumes, and nothing waits
   * on an answer nobody wants. The agreement check keeps its outcome (its manual check can
   * still be pressed), the CIP request does not.
   */
  constructor(source: InspectionSource<T>, timers: Timers, dropOnStop: boolean) {
    this.source = source
    this.timers = timers
    this.dropOnStop = dropOnStop
  }

  readonly getState = () => this.state
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  private set(next: Partial<InspectionState<T>>) {
    this.state = { ...this.state, ...next }
    for (const listener of this.listeners) listener()
  }

  /** Starts scheduling. A runner that is attached again (a remount) knows nothing yet and asks again. */
  attach() {
    this.scheduler = new CheckScheduler(() => this.check(), this.timers)
    this.asked = null
    this.dragging = false
    this.update(this.source)
  }

  /** Stops scheduling and retires a request in flight: its answer is dropped and nothing is left "running". */
  detach() {
    this.scheduler?.stop()
    this.scheduler = null
    this.run += 1
    if (this.state.running) this.set({ running: false })
  }

  /** The source of the render; asks, keeps or stops as `askDecision` says. */
  update(source: InspectionSource<T>) {
    this.source = source
    const scheduler = this.scheduler
    if (scheduler === null) return
    const next = { document: source.document, generation: source.generation, active: source.active && source.allowed }
    // An unchanged manual-mode render must keep Check now's pending timer.
    // Losing eligibility still cancels it, even if automatic checking was already off.
    const decision = source.allowed ? askDecision(this.asked, next) : 'stop'
    const wasActive = this.asked?.active ?? false
    this.asked = next
    if (decision === 'stop') {
      scheduler.stop()
      if (this.dropOnStop && wasActive) {
        this.run += 1
        this.set({ outcome: null, running: false })
      }
    } else if (decision === 'ask') {
      scheduler.changed()
    }
    if (source.dragging !== this.dragging) {
      this.dragging = source.dragging
      scheduler.pause(source.dragging)
    }
  }

  /** Asks now, however the automatic asking is set. */
  checkNow() {
    this.scheduler?.now()
  }

  /** What it asks about is the drawing on screen when the request starts, not when it was scheduled. */
  private async check() {
    const { document, generation, inspect, allowed } = this.source
    if (!allowed) return
    this.run += 1
    const run = this.run
    this.set({ running: true })
    let outcome: InspectionOutcome<T>
    try {
      outcome = { kind: 'ok', generation, inspection: await inspect(document.id) }
    } catch (failure) {
      const message = failure instanceof Error ? failure.message : 'The request failed.'
      const stop = failure instanceof ComputeStopped ? failure.stop : null
      outcome = { kind: 'failed', generation, snapshot: document, message, stop }
    }
    // A request that was retired (detached, or asking switched off) neither reports nor changes `running`:
    // the one that retired it already did.
    if (run === this.run) this.set({ outcome, running: false })
  }
}
