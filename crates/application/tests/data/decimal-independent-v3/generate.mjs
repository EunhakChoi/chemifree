// Independent integer-fraction oracle for synthetic decimal comparisons. SPDX-License-Identifier: MIT
import { writeFileSync } from 'node:fs';

let state = 6148914691236517205n;
const mask = (1n << 64n) - 1n;
function next() {
  state = (state * 6364136223846793005n + 1442695040888963407n) & mask;
  return state;
}
function operand() {
  const magnitude = next() % 1000000007n;
  const sign = (next() & 1n) === 0n ? -1n : 1n;
  const exponent = Number(next() % 81n) - 40;
  return [sign * magnitude, exponent];
}
function fraction([coefficient, exponent]) {
  return exponent >= 0
    ? [coefficient * 10n ** BigInt(exponent), 1n]
    : [coefficient, 10n ** BigInt(-exponent)];
}
function compare(left, right) {
  const [a, b] = fraction(left);
  const [c, d] = fraction(right);
  const difference = a * d - c * b;
  return difference < 0n ? -1 : difference > 0n ? 1 : 0;
}
function format([coefficient, exponent]) {
  return `${coefficient}e${exponent}`;
}
const lines = [];
for (let index = 0; index < 384; index += 1) {
  const left = operand();
  const right = index % 4 === 0 ? [left[0] * 10n, left[1] - 1] : operand();
  lines.push(`${format(left)}\t${format(right)}\t${compare(left, right)}`);
}
writeFileSync(new URL('./vectors.tsv', import.meta.url), `${lines.join('\n')}\n`);
