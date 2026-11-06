/**
 * Records completed requests using response headers already delivered by Playwright.
 * Returns an idempotent stop function that removes only this observer's listeners.
 * No asynchronous response reads can remain pending when the page is closed.
 */
export function observeRequestLog(page, append, now = () => performance.now()) {
  const runStarted = now()
  const started = new WeakMap()
  const statuses = new WeakMap()
  const requested = (request) => started.set(request, now())
  const responded = (response) => statuses.set(response.request(), response.status())
  const note = (request, outcome) => {
    const ended = now()
    const at = started.get(request) ?? ended
    const { pathname, search } = new URL(request.url())
    append(`+${Math.round(at - runStarted)} ${request.method()} ${pathname}${search} ${outcome} ${Math.round(ended - at)}ms`)
    started.delete(request)
    statuses.delete(request)
  }
  const finished = (request) => {
    const status = statuses.get(request)
    if (status === undefined) throw new Error(`Completed request has no observed response status: ${request.url()}`)
    note(request, status)
  }
  const failed = (request) => note(request, request.failure()?.errorText)
  page.on('request', requested)
  page.on('response', responded)
  page.on('requestfinished', finished)
  page.on('requestfailed', failed)
  return () => {
    page.off('request', requested)
    page.off('response', responded)
    page.off('requestfinished', finished)
    page.off('requestfailed', failed)
  }
}
