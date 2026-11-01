import type { CtLoss } from '@/lib/document-contract'

/** The loss report of any conversion: what a format could not keep, and why. */
export function LossList({ losses }: { losses: CtLoss[] }) {
  if (losses.length === 0) return <p>No conversion losses were reported.</p>
  return <ul className="list-disc space-y-1 pl-4">
    {losses.map((loss, index) => <li key={`${loss.source_path}-${loss.feature}-${index}`}>
      {loss.source_path}: {loss.reason} ({loss.semantic_impact}, {loss.disposition})
    </li>)}
  </ul>
}
