import { ChevronDownIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import type { Execution, OperationCatalog, OperationSupport } from '@/lib/contract'

const EXECUTION_LABEL: Record<Execution, string> = {
  executable: 'Runs',
  requires_data: 'Needs data',
  not_implemented: 'Not implemented',
  rejected: 'Rejected',
}

function ExecutionBadge({ execution }: { execution: Execution }) {
  return (
    <Badge variant={execution === 'executable' ? 'secondary' : 'outline'} data-execution={execution}>
      {EXECUTION_LABEL[execution]}
    </Badge>
  )
}

/** A row of the table; `parent` names the operation a sub-operation belongs to. */
function SupportRow({ support, parent }: { support: OperationSupport; parent?: string }) {
  return (
    <TableRow data-operation={parent === undefined ? support.id : `${parent}/${support.id}`}>
      <TableCell className={parent === undefined ? undefined : 'pl-6'}>
        {parent !== undefined && <span className="sr-only">{parent}, sub-operation </span>}
        <code className="font-mono text-xs">{support.id}</code>
      </TableCell>
      <TableCell className="text-muted-foreground">
        {support.representation === 'supported' ? 'Supported' : 'Not supported'}
      </TableCell>
      <TableCell><ExecutionBadge execution={support.execution} /></TableCell>
      <TableCell className="text-muted-foreground">{support.followup ?? '—'}</TableCell>
      <TableCell className="min-w-64 text-xs whitespace-normal text-muted-foreground">{support.note}</TableCell>
    </TableRow>
  )
}

/**
 * Per-operation support from the operation contract's catalog. Representation and execution are
 * shown apart: an operation the contract can describe is not thereby run.
 */
export function OperationCatalogSection({ catalog }: { catalog: OperationCatalog | null }) {
  if (catalog === null) {
    return (
      <p className="text-xs text-muted-foreground">
        This service does not report per-operation support.
      </p>
    )
  }
  const running = catalog.operations.filter((operation) => operation.execution === 'executable').length
  return (
    <Collapsible className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-col gap-1">
          <h3 className="text-sm font-medium">Operation contract {catalog.contracts.join(', ')}</h3>
          <p className="text-xs text-muted-foreground" data-testid="operation-summary">
            {running} of {catalog.operations.length} operations run; the others report why not.
          </p>
        </div>
        <CollapsibleTrigger render={<Button variant="outline" size="sm" />} className="group">
          Operations
          <ChevronDownIcon
            data-icon="inline-end"
            aria-hidden="true"
            className="transition-transform group-data-[panel-open]:rotate-180"
          />
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent className="flex flex-col gap-4">
        <Table aria-label="Operations">
          <TableHeader>
            <TableRow>
              <TableHead>Operation</TableHead>
              <TableHead>Representation</TableHead>
              <TableHead>Execution</TableHead>
              <TableHead>Follow-up</TableHead>
              <TableHead>Note</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {catalog.operations.map((operation) => [
              <SupportRow key={operation.id} support={operation} />,
              ...(operation.sub_operations ?? []).map((sub) => (
                <SupportRow key={`${operation.id}/${sub.id}`} support={sub} parent={operation.id} />
              )),
            ])}
          </TableBody>
        </Table>
        <ul className="flex flex-col gap-2">
          {catalog.profiles.map((profile) => (
            <li key={profile.id} className="flex flex-wrap items-center gap-2 text-xs" data-profile={profile.id}>
              <code className="font-mono">{profile.id}</code>
              <ExecutionBadge execution={profile.execution} />
              <span className="text-muted-foreground">{profile.note}</span>
            </li>
          ))}
        </ul>
      </CollapsibleContent>
    </Collapsible>
  )
}
