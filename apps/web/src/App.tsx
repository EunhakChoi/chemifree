import {
  ArrowUpRightIcon,
  CircleCheckIcon,
  CircleDashedIcon,
  FlaskConicalIcon,
  CodeIcon,
  HexagonIcon,
  LayoutDashboardIcon,
  ListChecksIcon,
  RadioIcon,
  RefreshCwIcon,
  ServerIcon,
  TerminalIcon,
  UnplugIcon,
} from 'lucide-react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { DocumentWorkspace } from '@/components/document-workspace'
import { OperationCatalogSection } from '@/components/operation-catalog'
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { useRuntime, type RuntimeState } from '@/hooks/use-runtime'

const repositoryUrl = 'https://github.com/EunhakChoi/chemifree'

function RuntimeCard({ state, refresh }: { state: RuntimeState; refresh: () => void }) {
  return (
    <Card id="runtime" className="scroll-mt-6">
      <CardHeader>
        <CardTitle><h2>Local runtime</h2></CardTitle>
        <CardDescription>Live status from your local service.</CardDescription>
        <CardAction><ServerIcon aria-hidden="true" className="size-4" /></CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-5" aria-live="polite" aria-busy={state.kind === 'loading'}>
        {state.kind === 'loading' && (
          <div className="flex flex-col gap-3" role="status">
            <span className="text-sm text-muted-foreground">Connecting to local service…</span>
            <Skeleton className="h-6 w-32" />
            <Skeleton className="h-4 w-48 max-w-full" />
          </div>
        )}
        {state.kind === 'ready' && (
          <>
            <div className="flex items-center gap-2">
              <CircleCheckIcon aria-hidden="true" className="size-5 text-primary" />
              <p className="font-medium">Service connected</p>
              <Badge variant="secondary">{state.snapshot.health.status}</Badge>
            </div>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <dt className="text-muted-foreground">Version</dt>
              <dd className="text-right">{state.snapshot.health.version}</dd>
              <dt className="text-muted-foreground">API</dt>
              <dd className="text-right">{state.snapshot.health.api_version}</dd>
              <dt className="text-muted-foreground">Stage</dt>
              <dd className="text-right">{state.snapshot.health.stage}</dd>
              <dt className="text-muted-foreground">Checked</dt>
              <dd className="text-right">{state.snapshot.checkedAt.toLocaleTimeString()}</dd>
            </dl>
            <p className="text-xs leading-relaxed text-muted-foreground">
              This confirms service liveness. Scientific tools are listed separately below.
            </p>
          </>
        )}
        {state.kind === 'error' && (
          <>
            <Alert variant="destructive">
              <UnplugIcon aria-hidden="true" />
              <AlertTitle>Local service unavailable</AlertTitle>
              <AlertDescription>{state.message}</AlertDescription>
            </Alert>
            <p className="text-sm leading-relaxed text-muted-foreground">
              Start the workspace with <code className="font-mono text-xs">npm run dev</code>, then retry.
            </p>
          </>
        )}
      </CardContent>
      <CardFooter>
        <Button variant="outline" onClick={refresh} disabled={state.kind === 'loading'} className="w-full">
          <RefreshCwIcon data-icon="inline-start" aria-hidden="true" />
          {state.kind === 'error' ? 'Retry connection' : 'Refresh status'}
        </Button>
      </CardFooter>
    </Card>
  )
}

function CapabilitiesCard({ state }: { state: RuntimeState }) {
  return (
    <Card id="capabilities" className="scroll-mt-6">
      <CardHeader>
        <CardTitle><h2>What is available</h2></CardTitle>
        <CardDescription>Implementation status reported by the application.</CardDescription>
        <CardAction><Badge variant="outline">API v1</Badge></CardAction>
      </CardHeader>
      <CardContent aria-live="polite" aria-busy={state.kind === 'loading'}>
        {state.kind === 'loading' && (
          <div className="flex flex-col gap-5" role="status">
            <p className="sr-only">Loading capabilities</p>
            {[0, 1, 2].map((item) => (
              <div key={item} className="flex items-center justify-between gap-4">
                <div className="flex flex-col gap-2">
                  <Skeleton className="h-4 w-36" />
                  <Skeleton className="h-3 w-64 max-w-full" />
                </div>
                <Skeleton className="hidden h-5 w-24 sm:block" />
              </div>
            ))}
          </div>
        )}
        {state.kind === 'error' && (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon"><RadioIcon aria-hidden="true" /></EmptyMedia>
              <EmptyTitle>Waiting for the local service</EmptyTitle>
              <EmptyDescription>Reconnect to see the actual capability list.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
        {state.kind === 'ready' && (
          <div className="flex flex-col gap-6">
            <ul className="grid gap-x-8 gap-y-6 md:grid-cols-2">
              {state.snapshot.capabilities.map((capability) => (
                <li key={capability.id} className="flex items-start gap-3">
                  {capability.status === 'available'
                    ? <CircleCheckIcon aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-primary" />
                    : <CircleDashedIcon aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-muted-foreground" />}
                  <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <h3 className="text-sm font-medium">{capability.label}</h3>
                      <Badge variant={capability.status === 'available' ? 'secondary' : 'outline'}>
                        {capability.status === 'available' ? 'Available' : 'Not implemented'}
                      </Badge>
                    </div>
                    <p className="text-xs leading-relaxed text-muted-foreground">{capability.description}</p>
                  </div>
                </li>
              ))}
            </ul>
            <Separator />
            <OperationCatalogSection catalog={state.snapshot.operationCatalog} />
          </div>
        )}
      </CardContent>
      <CardFooter>
        <p className="text-xs leading-relaxed text-muted-foreground">
          The web UI and CLI use the same application service. No chemical analysis engine or model data is included.
        </p>
      </CardFooter>
    </Card>
  )
}

function App() {
  const { state, refresh } = useRuntime()

  return (
    <div className="min-h-svh">
      <a href="#main" className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:rounded-lg focus:bg-background focus:p-3 focus:text-foreground">
        Skip to content
      </a>
      <header className="border-b bg-card">
        <div className="mx-auto flex max-w-7xl items-center justify-between gap-4 px-5 py-5 sm:px-8">
          <a href="#workspace" className="flex items-center gap-3" aria-label="Chemifree workspace">
            <span className="flex size-9 items-center justify-center rounded-xl bg-primary text-primary-foreground">
              <HexagonIcon aria-hidden="true" className="size-5" />
            </span>
            <span className="text-lg font-semibold tracking-tight">Chemifree</span>
            <Badge variant="secondary" className="hidden sm:inline-flex">Early preview</Badge>
          </a>
          <a className={buttonVariants({ variant: 'outline', size: 'sm' })} href={repositoryUrl} target="_blank" rel="noreferrer">
            <CodeIcon data-icon="inline-start" aria-hidden="true" />
            GitHub
            <ArrowUpRightIcon data-icon="inline-end" aria-hidden="true" />
          </a>
        </div>
      </header>

      <div className="mx-auto grid max-w-7xl gap-8 px-5 py-8 sm:px-8 lg:grid-cols-[160px_minmax(0,1fr)] lg:gap-10 lg:py-10">
        <aside>
          <nav aria-label="Workspace sections" className="flex flex-wrap gap-2 lg:sticky lg:top-8 lg:flex-col lg:gap-1">
            <a className={buttonVariants({ variant: 'secondary', className: 'justify-start' })} href="#workspace">
              <LayoutDashboardIcon data-icon="inline-start" aria-hidden="true" />Workspace
            </a>
            <a className={buttonVariants({ variant: 'ghost', className: 'justify-start' })} href="#runtime">
              <ServerIcon data-icon="inline-start" aria-hidden="true" />Runtime
            </a>
            <a className={buttonVariants({ variant: 'ghost', className: 'justify-start' })} href="#capabilities">
              <ListChecksIcon data-icon="inline-start" aria-hidden="true" />Capabilities
            </a>
          </nav>
        </aside>

        <main id="main" className="flex min-w-0 flex-col gap-7">
          <section id="workspace" aria-labelledby="workspace-title" className="flex scroll-mt-6 flex-col gap-3">
            <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-widest text-primary">
              <FlaskConicalIcon aria-hidden="true" className="size-3.5" />Open chemistry, in progress
            </div>
            <h1 id="workspace-title" className="text-3xl font-semibold tracking-tight sm:text-4xl">A clear space for chemistry.</h1>
            <p className="max-w-2xl text-sm leading-relaxed text-muted-foreground sm:text-base">
              An open-source chemical drawing workspace for humans and agents. Draw, undo, save, and reopen structures locally; chemistry analysis comes later.
            </p>
          </section>

          <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_300px]">
            <DocumentWorkspace />

            <div className="flex min-w-0 flex-col gap-5">
              <RuntimeCard state={state} refresh={refresh} />
              <Card size="sm">
                <CardHeader>
                  <CardTitle><h2>Also at your terminal</h2></CardTitle>
                  <CardDescription>Inspect and edit the same documents directly.</CardDescription>
                  <CardAction><TerminalIcon aria-hidden="true" className="size-4" /></CardAction>
                </CardHeader>
                <CardContent>
                  <code className="block break-words rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">cargo run -p chemifree-cli -- documents list</code>
                </CardContent>
                <CardFooter><p className="text-xs text-muted-foreground">Runs locally without an HTTP server, on the same data directory.</p></CardFooter>
              </Card>
            </div>
          </div>

          <CapabilitiesCard state={state} />

          <footer className="flex flex-wrap items-center justify-between gap-3 pb-2 text-xs text-muted-foreground">
            <p>Chemifree · Independently authored · MIT</p>
            <p>Local-first workspace · v0.1.0</p>
          </footer>
        </main>
      </div>
    </div>
  )
}

export default App
