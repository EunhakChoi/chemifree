// Keeps Playwright's request interception on for a whole browser context.
//
// Playwright intercepts requests only while at least one route is registered. When the last
// page route goes away (a `times: 1` route that has fired, or `unroute`), playwright-core
// 1.56.1 turns interception off. A request the page starts in that moment is already paused
// by the browser, but Playwright no longer continues it, so it never reaches the server and
// fails only when the page gives up on it. A context route that lets every request through
// keeps interception on, so adding or removing page routes never toggles it.

/** Registers a pass-through route on `context` for as long as the context lives. */
export async function keepRequestInterception(context) {
  await context.route('**/*', (route) => route.continue())
}
