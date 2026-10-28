// Module resolve hook that maps the web app's `@/…` import alias to apps/web/src/…, so Node
// can load the app's TypeScript modules directly (with its built-in type stripping).
const source = new URL('../../apps/web/src/', import.meta.url)

export async function resolve(specifier, context, nextResolve) {
  if (specifier.startsWith('@/')) {
    return nextResolve(new URL(`${specifier.slice(2)}.ts`, source).href, context)
  }
  return nextResolve(specifier, context)
}
