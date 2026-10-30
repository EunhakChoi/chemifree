import { writeFileSync } from 'node:fs'

let seed = 0x1743a5c9
const next = () => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
  return seed
}
const pow10 = (n) => 10n ** BigInt(n)
function sample() {
  const number = next() % 1000000
  const fraction = next() % 6
  const exponent = (next() % 13) - 6
  const negative = ((next() >>> 8) & 1) !== 0
  const digits = String(number).padStart(fraction + 1, '0')
  const mantissa = fraction ? `${digits.slice(0, -fraction)}.${digits.slice(-fraction)}` : digits
  const scale = exponent - fraction
  return {
    token: `${negative ? '-' : ''}${mantissa}e${exponent}`,
    numerator: BigInt(number) * (negative ? -1n : 1n) * pow10(Math.max(scale, 0)),
    denominator: pow10(Math.max(-scale, 0)),
  }
}
const comparisons = []
for (let index = 0; index < 300; index += 1) {
  const left = sample()
  const right = index % 7 === 0 ? left : sample()
  const a = left.numerator * right.denominator
  const b = right.numerator * left.denominator
  comparisons.push({ left: left.token, right: right.token, ordering: a < b ? -1 : a > b ? 1 : 0 })
}
writeFileSync(new URL('rational-order.json', import.meta.url), `${JSON.stringify(comparisons)}\n`)
