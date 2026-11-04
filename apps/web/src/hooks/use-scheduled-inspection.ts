import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'

import { InspectionRunner, type InspectionOutcome, type InspectionSource } from '@/editor/scheduled-inspection'

const browserTimers = {
  set: (task: () => void, milliseconds: number) => window.setTimeout(task, milliseconds),
  clear: (handle: unknown) => window.clearTimeout(handle as number),
}

/**
 * Runs a read-only inspection of the drawing on screen as `InspectionRunner` schedules it
 * (a wait after the last change, one request in flight, none while dragging) and returns the
 * last outcome. Shared by the agreement check and the CIP request so that both follow the same
 * rules, including a remount: detaching retires a request in flight and attaching again asks again.
 */
export function useScheduledInspection<T>(
  source: InspectionSource<T>,
  dropOnStop: boolean,
): { outcome: InspectionOutcome<T> | null; running: boolean; checkNow: () => void } {
  const [runner] = useState(() => new InspectionRunner(source, browserTimers, dropOnStop))
  // Declared first, so the update of a render finds the runner attached.
  useEffect(() => {
    runner.attach()
    return () => runner.detach()
  }, [runner])
  useEffect(() => {
    runner.update(source)
  })
  const { outcome, running } = useSyncExternalStore(runner.subscribe, runner.getState)
  const checkNow = useCallback(() => runner.checkNow(), [runner])
  return { outcome, running, checkNow }
}
