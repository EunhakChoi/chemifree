// Keeps Playwright's request interception on for a whole browser run.
//
// Playwright intercepts requests only while some route is registered. When the last page route
// goes away (a `times: 1` route that fired, or `unroute`), it switches interception off. A
// request the page starts at that moment can already be paused in the browser while Playwright
// treats it as not intercepted, so nothing ever continues it: it never reaches the server and
// fails only when the page aborts it (#8). A context route that passes every request through
// keeps interception on, so adding and removing step routes never toggles it.

/** Registers a pass-through route on `context`; call before the first page is created. */
export async function keepRequestInterception(context) {
  await context.route('**/*', (route) => route.continue())
}
