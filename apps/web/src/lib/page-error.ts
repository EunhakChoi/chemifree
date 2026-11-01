/**
 * The page error after a queued task settles. A success clears an error the user had already
 * seen, never one from a request that failed while the task waited. A failure becomes the
 * page error unless its caller handles it (a panel that shows its own failures), in which
 * case the page error is left exactly as it was: only a success may clear it.
 */
export function errorAfterTask<E>(
  previous: E | null,
  settled: { failed: false; newerFailure: boolean } | { failed: true; handled: boolean; error: E },
): E | null {
  if (!settled.failed) return settled.newerFailure ? previous : null
  return settled.handled ? previous : settled.error
}
