/** Parse JSON without duplicate decoded object keys. Callers apply their input byte limit first. */
export function parseStrictJson(text: string): unknown {
  // Let the native parser validate grammar; the scan only checks information it discards.
  const value: unknown = JSON.parse(text)
  const scopes: Array<Set<string> | null> = []
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index]
    if (character === '{') scopes.push(new Set<string>())
    else if (character === '[') scopes.push(null)
    else if (character === '}' || character === ']') scopes.pop()
    else if (character === '"') {
      const start = index
      for (index += 1; index < text.length; index += 1) {
        if (text[index] === '\\') index += 1
        else if (text[index] === '"') break
      }
      let next = index + 1
      while (/[ \t\r\n]/.test(text[next] ?? '')) next += 1
      // In valid JSON, only an object key can be followed by a colon.
      if (text[next] !== ':') continue
      const keys = scopes.at(-1)
      const key: unknown = JSON.parse(text.slice(start, index + 1))
      if (!(keys instanceof Set) || typeof key !== 'string') throw new SyntaxError('Invalid JSON object key.')
      if (keys.has(key)) throw new SyntaxError('Duplicate JSON object key.')
      keys.add(key)
    }
  }
  return value
}
