/**
 * The page's colour theme: the preference chosen (`system` until preferences are read), and
 * for `system` the operating system's light or dark setting, followed as it changes.
 */
import type { Theme } from '@/lib/document-contract'

const colorScheme = window.matchMedia('(prefers-color-scheme: dark)')
let chosen: Theme = 'system'

function update() {
  const dark = chosen === 'dark' || (chosen === 'system' && colorScheme.matches)
  document.documentElement.classList.toggle('dark', dark)
}

colorScheme.addEventListener('change', update)

export function applyTheme(theme: Theme) {
  chosen = theme
  update()
}
