import { useEffect, useState, type KeyboardEvent, type ReactNode } from 'react'
import {
  ArchiveRestoreIcon,
  AtomIcon,
  CopyPlusIcon,
  EraserIcon,
  FilePlus2Icon,
  FolderOpenIcon,
  HexagonIcon,
  MinusIcon,
  MousePointer2Icon,
  PlusIcon,
  Redo2Icon,
  RotateCwIcon,
  SaveIcon,
  SlashIcon,
  Trash2Icon,
  TriangleAlertIcon,
  Undo2Icon,
  XIcon,
} from 'lucide-react'

import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Badge } from '@/components/ui/badge'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Kbd } from '@/components/ui/kbd'
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { stillApplicable } from '@/editor/commands'
import { StructureCanvas, type Tool } from '@/editor/structure-canvas'
import type { AutosaveState } from '@/hooks/use-autosave'
import { useDocumentEditor, type CopyNotice, type DocumentEditor } from '@/hooks/use-document-editor'
import type {
  AtomView,
  BondOrder,
  Command,
  DocumentResponse,
  DocumentView,
  RecoveryView,
} from '@/lib/document-contract'

const ELEMENTS = ['C', 'H', 'N', 'O', 'S', 'P', 'F', 'Cl', 'Br', 'I']
const TOOLS: { value: Tool; label: string; key: string; icon: ReactNode }[] = [
  { value: 'select', label: 'Select and move', key: 'V', icon: <MousePointer2Icon /> },
  { value: 'atom', label: 'Atom', key: 'A', icon: <AtomIcon /> },
  { value: 'bond', label: 'Bond', key: 'B', icon: <SlashIcon /> },
  { value: 'erase', label: 'Erase', key: 'E', icon: <EraserIcon /> },
]
const ORDERS: { value: BondOrder; label: string; glyph: string }[] = [
  { value: 'single', label: 'Single bond', glyph: '—' },
  { value: 'double', label: 'Double bond', glyph: '=' },
  { value: 'triple', label: 'Triple bond', glyph: '≡' },
]
const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
const MOD = isMac ? '⌘' : 'Ctrl'

function Hint({ label, keys, children }: { label: string; keys?: string; children: React.ReactElement }) {
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipContent>
        {label}
        {keys && <Kbd>{keys}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}

/** Applies a text field's value on Enter or blur when it changed. */
function commitOnEnter(event: KeyboardEvent<HTMLInputElement>) {
  if (event.key === 'Enter') event.currentTarget.blur()
}

export function DocumentWorkspace() {
  const editor = useDocumentEditor()
  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle><h2>Drawing workspace</h2></CardTitle>
        <CardDescription>
          {editor.current ? 'Edits apply on the local service; save to keep them.' : 'Create a document or open a saved one.'}
        </CardDescription>
        <CardAction>
          {editor.busy ? <Spinner /> : <Badge variant="outline">Local</Badge>}
        </CardAction>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-4">
        {editor.error && <ErrorNotice editor={editor} />}
        {editor.current ? <Editor editor={editor} current={editor.current} /> : <DocumentPicker editor={editor} />}
      </CardContent>
      <CardFooter className="gap-2">
        <p className="text-xs leading-relaxed text-muted-foreground">
          Drawing only: Chemifree records atoms and bonds as drawn. Valence, aromaticity, stereo, and naming are not computed.
        </p>
      </CardFooter>
    </Card>
  )
}

/** Errors that report a state to act on rather than a failure. */
const INFORMATIONAL = new Set([
  'invalid_command',
  'create_unconfirmed',
  'copy_unconfirmed',
  'recovery_pending',
  'draft_outdated',
  'draft_in_use',
  'draft_not_found',
])

function ErrorNotice({ editor }: { editor: DocumentEditor }) {
  const error = editor.error!
  const stale = error.code === 'revision_conflict' || error.code === 'service_restarted'
  const title = {
    revision_conflict: 'The document changed',
    service_restarted: 'Local service restarted',
    create_unconfirmed: 'New document not confirmed',
    copy_unconfirmed: 'Copy not confirmed',
    recovery_pending: 'Earlier unsaved work is waiting',
    draft_outdated: 'Saved elsewhere since this draft',
    draft_in_use: 'Draft in use',
    draft_not_found: 'No recovery draft',
    save_conflict: 'Saved elsewhere',
    storage_failed: 'Save failed',
    invalid_command: 'Edit rejected',
    unreachable: 'Local service unavailable',
  }[error.code] ?? 'Request failed'
  return (
    <Alert variant={stale || INFORMATIONAL.has(error.code) ? 'default' : 'destructive'}>
      <TriangleAlertIcon aria-hidden="true" />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        {error.message}
      </AlertDescription>
      <AlertAction>
        {stale ? (
          <Button size="sm" variant="outline" onClick={() => void editor.reload()}>
            <RotateCwIcon data-icon="inline-start" aria-hidden="true" />Reload
          </Button>
        ) : (
          <Button size="sm" variant="ghost" onClick={editor.dismissError}>Dismiss</Button>
        )}
      </AlertAction>
    </Alert>
  )
}

function DocumentPicker({ editor }: { editor: DocumentEditor }) {
  const { list } = editor
  return (
    <div className="flex flex-col gap-4">
      <Empty className="min-h-56 border">
        <EmptyHeader>
          <EmptyMedia variant="icon"><HexagonIcon aria-hidden="true" /></EmptyMedia>
          <EmptyTitle>No document open</EmptyTitle>
          <EmptyDescription>Draw atoms and bonds, undo and redo, and save to a local file.</EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button onClick={() => void editor.create()} disabled={editor.busy}>
            <FilePlus2Icon data-icon="inline-start" aria-hidden="true" />New document
          </Button>
        </EmptyContent>
      </Empty>
      <section aria-labelledby="saved-documents" className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-2">
          <h3 id="saved-documents" className="text-sm font-medium">Saved documents</h3>
          <Button size="sm" variant="ghost" onClick={editor.refreshList} disabled={list.kind === 'loading'}>
            <RotateCwIcon data-icon="inline-start" aria-hidden="true" />Refresh
          </Button>
        </div>
        {list.kind === 'loading' && <Skeleton className="h-10 w-full" />}
        {list.kind === 'error' && <p className="text-sm text-muted-foreground">{list.message}</p>}
        {list.kind === 'ready' && list.list.documents.length === 0 && (
          <p className="text-sm text-muted-foreground">Nothing saved yet.</p>
        )}
        {list.kind === 'ready' && list.list.documents.length > 0 && (
          <ul className="flex flex-col divide-y rounded-lg border">
            {list.list.documents.map((item) => (
              <li key={item.id} data-document-id={item.id} className="flex items-center justify-between gap-3 p-3">
                <div className="flex min-w-0 flex-col gap-0.5">
                  <span className="truncate text-sm font-medium">{item.title}</span>
                  <span className="text-xs text-muted-foreground">
                    {item.atom_count} atoms · {item.bond_count} bonds · revision {item.revision}
                  </span>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {item.unsaved_changes && <Badge variant="secondary">Unsaved</Badge>}
                  {item.recovery && <Badge variant="outline">{RECOVERY_BADGES[item.recovery.state]}</Badge>}
                  <Button size="sm" variant="outline" onClick={() => void editor.open(item.id)} disabled={editor.busy}>
                    <FolderOpenIcon data-icon="inline-start" aria-hidden="true" />Open
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
        {list.kind === 'ready' && list.list.unreadable.length > 0 && (
          <Alert variant="destructive">
            <TriangleAlertIcon aria-hidden="true" />
            <AlertTitle>Unreadable files</AlertTitle>
            <AlertDescription>
              {list.list.unreadable.map((item) => `${item.id}: ${item.message}`).join(' · ')}
            </AlertDescription>
          </Alert>
        )}
      </section>
    </div>
  )
}

function Editor({ editor, current }: { editor: DocumentEditor; current: DocumentResponse }) {
  const { document, session } = current
  const [tool, setTool] = useState<Tool>('bond')
  const [element, setElement] = useState('C')
  const [bondOrder, setBondOrder] = useState<BondOrder>('single')
  const [selection, setSelection] = useState<Set<string>>(new Set())
  const exists = new Set([...document.atoms.map((atom) => atom.id), ...document.bonds.map((bond) => bond.id)])
  const selected = new Set([...selection].filter((id) => exists.has(id)))
  const selectedIds = [...selected]

  function deleteSelection() {
    const atoms = selectedIds.filter((id) => id.startsWith('a'))
    const bonds = selectedIds.filter((id) => id.startsWith('b'))
    const removed = new Set(atoms)
    const remainingBonds = bonds.filter((id) => {
      const bond = document.bonds.find((candidate) => candidate.id === id)
      return bond && !removed.has(bond.begin) && !removed.has(bond.end)
    })
    void editor.apply([
      ...(atoms.length ? [{ op: 'delete_atoms' as const, atoms }] : []),
      ...(remainingBonds.length ? [{ op: 'delete_bonds' as const, bonds: remainingBonds }] : []),
    ])
    setSelection(new Set())
  }

  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, [contenteditable="true"], [role="dialog"], [role="alertdialog"]')) return
      const mod = event.metaKey || event.ctrlKey
      const key = event.key.toLowerCase()
      if (mod && key === 'z') {
        event.preventDefault()
        if (event.shiftKey) {
          if (session.redo_depth > 0) void editor.redo()
        } else if (session.undo_depth > 0) void editor.undo()
      } else if (mod && key === 'y') {
        event.preventDefault()
        if (session.redo_depth > 0) void editor.redo()
      } else if (mod && key === 's') {
        event.preventDefault()
        void editor.save()
      } else if (!mod && !event.altKey) {
        const shortcut = TOOLS.find((item) => item.key.toLowerCase() === key)
        if (shortcut) setTool(shortcut.value)
        else if (key === 'escape') setSelection(new Set())
        else if ((key === 'delete' || key === 'backspace') && selected.size > 0) {
          event.preventDefault()
          deleteSelection()
        }
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          key={`${document.id}:${document.title}`}
          aria-label="Document title"
          defaultValue={document.title}
          maxLength={200}
          className="w-full max-w-64 font-medium"
          onKeyDown={commitOnEnter}
          onBlur={(event) => {
            const title = event.currentTarget.value.trim()
            if (title && title !== document.title) void editor.apply([{ op: 'rename', title }])
            else event.currentTarget.value = document.title
          }}
        />
        <Badge variant="outline" aria-label={`Revision ${document.revision}`}>r{document.revision}</Badge>
        <Badge variant={session.unsaved_changes ? 'secondary' : 'outline'} data-testid="save-state">
          {session.unsaved_changes ? 'Unsaved changes' : 'Saved'}
        </Badge>
        <div className="ml-auto flex items-center gap-1">
          <Hint label="Undo" keys={`${MOD}+Z`}>
            <Button size="icon" variant="ghost" aria-label="Undo" onClick={() => void editor.undo()} disabled={session.undo_depth === 0}>
              <Undo2Icon aria-hidden="true" />
            </Button>
          </Hint>
          <Hint label="Redo" keys={`${MOD}+Shift+Z`}>
            <Button size="icon" variant="ghost" aria-label="Redo" onClick={() => void editor.redo()} disabled={session.redo_depth === 0}>
              <Redo2Icon aria-hidden="true" />
            </Button>
          </Hint>
          <Hint label="Save" keys={`${MOD}+S`}>
            <Button size="sm" variant={session.unsaved_changes ? 'default' : 'outline'} onClick={() => void editor.save()}>
              <SaveIcon data-icon="inline-start" aria-hidden="true" />Save
            </Button>
          </Hint>
          <SaveAsButton editor={editor} document={document} unsaved={session.unsaved_changes} />
          <CloseButton editor={editor} unsaved={session.unsaved_changes} />
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span data-testid="file-state">
          File: {session.unsaved_changes ? `last saved at revision ${session.saved_revision}` : `saved at revision ${document.revision}`}
        </span>
        <DraftStatus state={editor.autosave.state} onRetry={editor.autosave.retryNow} />
      </div>
      {current.recovery && <RecoveryNotice editor={editor} recovery={current.recovery} unsaved={session.unsaved_changes} />}
      {editor.copyNotice && <CopyNoticeBanner editor={editor} notice={editor.copyNotice} title={document.title} />}

      <div className="flex flex-wrap items-center gap-2">
        <ToggleGroup
          aria-label="Tool"
          variant="outline"
          size="sm"
          value={[tool]}
          onValueChange={(values) => values[0] && setTool(values[0] as Tool)}
        >
          {TOOLS.map((item) => (
            <Hint key={item.value} label={item.label} keys={item.key}>
              <ToggleGroupItem value={item.value} aria-label={item.label}>{item.icon}</ToggleGroupItem>
            </Hint>
          ))}
        </ToggleGroup>
        <Separator orientation="vertical" className="h-6" />
        <ToggleGroup
          aria-label="Element for the atom tool"
          variant="outline"
          size="sm"
          spacing={0}
          value={[element]}
          onValueChange={(values) => {
            if (!values[0]) return
            setElement(values[0])
            setTool('atom')
          }}
        >
          {ELEMENTS.map((symbol) => (
            <ToggleGroupItem key={symbol} value={symbol} aria-label={`Element ${symbol}`} className="min-w-8 font-mono">
              {symbol}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <Separator orientation="vertical" className="h-6" />
        <ToggleGroup
          aria-label="Bond order for new bonds"
          variant="outline"
          size="sm"
          spacing={0}
          value={[bondOrder]}
          onValueChange={(values) => {
            if (!values[0]) return
            setBondOrder(values[0] as BondOrder)
            setTool('bond')
          }}
        >
          {ORDERS.map((order) => (
            <ToggleGroupItem key={order.value} value={order.value} aria-label={order.label} className="min-w-8 font-mono">
              {order.glyph}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      <div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(0,1fr)_220px]">
        <div className="aspect-[5/3] min-w-0">
          <StructureCanvas
            document={document}
            tool={tool}
            element={element}
            bondOrder={bondOrder}
            selection={selected}
            onSelectionChange={setSelection}
            onCommands={(commands) => void editor.apply((latest) => stillApplicable(commands, latest))}
            disabled={editor.busy}
          />
        </div>
        <Inspector document={document} selected={selectedIds} editor={editor} onDelete={deleteSelection} />
      </div>
      <p className="text-xs leading-relaxed text-muted-foreground">
        Bond tool: click an atom to grow a chain, drag to bond, click a bond to cycle its order. Atom tool: click empty space
        to place, click an atom to change its element. Select: drag atoms to move, drag empty space to box-select.
      </p>
    </div>
  )
}

const RECOVERY_BADGES: Record<RecoveryView['state'], string> = {
  recoverable: 'Unsaved work found',
  outdated: 'Old unsaved work',
  unreadable: 'Unreadable draft',
  in_use: 'Draft in use',
}

const time = (ms: number | null) => (ms === null ? 'an earlier session' : new Date(ms).toLocaleTimeString())

/** The recovery draft's state, apart from the saved file's. */
function DraftStatus({ state, onRetry }: { state: AutosaveState; onRetry: () => void }) {
  const text = {
    clean: 'not needed',
    waiting: 'pending…',
    saving: 'saving…',
    saved: state.kind === 'saved' ? `saved ${time(state.at)} (revision ${state.revision})` : '',
    failed: state.kind === 'failed' ? `failed, retrying: ${state.message}` : '',
    paused: state.kind === 'paused' ? `paused: ${state.message}` : '',
  }[state.kind]
  return (
    <span className="flex min-w-0 items-center gap-1.5" data-testid="draft-state" data-state={state.kind}>
      {state.kind === 'saving' && <Spinner className="size-3" />}
      <span className="min-w-0 break-words">Recovery draft: {text}</span>
      {state.kind === 'failed' && (
        <Button size="xs" variant="outline" onClick={onRetry}>Retry now</Button>
      )}
    </span>
  )
}

function RecoveryNotice({ editor, recovery, unsaved }: { editor: DocumentEditor; recovery: RecoveryView; unsaved: boolean }) {
  const when = time(recovery.saved_at_ms)
  const copy = {
    recoverable: {
      title: 'Unsaved work found',
      description: `A session that ended without saving left changes from ${when}${recovery.revision === null ? '' : ` (revision ${recovery.revision})`}. Recover them to continue editing with their undo history, or discard them.${unsaved ? ' Save or discard the changes open now first.' : ''}`,
    },
    outdated: {
      title: 'Unsaved work from an older version',
      description: `Changes from ${when} were made on revision ${recovery.base_revision ?? '?'}, but the document was saved elsewhere since. Recover them as a separate copy, or discard them.`,
    },
    unreadable: {
      title: 'Unreadable recovery draft',
      description: `A recovery draft exists but cannot be read${recovery.message ? `: ${recovery.message}` : ''}. It can only be discarded.`,
    },
    in_use: {
      title: 'Draft held by another process',
      description: 'Another running Chemifree process holds unsaved work for this document. Finish there, or close that process to recover it here.',
    },
  }[recovery.state]
  return (
    <Alert data-testid="recovery-notice" data-state={recovery.state}>
      <ArchiveRestoreIcon aria-hidden="true" />
      <AlertTitle>{copy.title}</AlertTitle>
      <AlertDescription>{copy.description}</AlertDescription>
      {recovery.state !== 'in_use' && (
        <AlertAction className="flex flex-wrap gap-2">
          {recovery.state === 'recoverable' && (
            <Button size="sm" onClick={() => void editor.recover()} disabled={unsaved || editor.busy}>Recover</Button>
          )}
          {recovery.state === 'outdated' && (
            <Button size="sm" onClick={() => void editor.recoverAsCopy()} disabled={editor.busy}>Recover as copy</Button>
          )}
          <DiscardDraftButton editor={editor} when={when} />
        </AlertAction>
      )}
    </Alert>
  )
}

function DiscardDraftButton({ editor, when }: { editor: DocumentEditor; when: string }) {
  return (
    <AlertDialog>
      <AlertDialogTrigger render={<Button size="sm" variant="outline" disabled={editor.busy}>Discard draft</Button>} />
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Discard the unsaved work?</AlertDialogTitle>
          <AlertDialogDescription>
            The recovery draft from {when} will be deleted. The saved document is not changed. This cannot be undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep it</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => void editor.discardDraft()}>Discard draft</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function CopyNoticeBanner({ editor, notice, title }: { editor: DocumentEditor; notice: CopyNotice; title: string }) {
  const original = `“${notice.sourceTitle}”`
  return (
    <Alert data-testid="copy-notice">
      <CopyPlusIcon aria-hidden="true" />
      <AlertTitle>Now editing the copy “{title}”</AlertTitle>
      <AlertDescription>
        {notice.recovered
          ? `The recovered work was saved as this new document. ${original} is unchanged.`
          : `This is a new, separate document. ${original} keeps its last save${notice.discardedChanges ? '; the changes that were unsaved there are only in this copy' : ''}.`}
      </AlertDescription>
      <AlertAction>
        <Button size="sm" variant="outline" onClick={() => void editor.open(notice.sourceId)} disabled={editor.busy}>
          Open original
        </Button>
      </AlertAction>
    </Alert>
  )
}

function SaveAsButton({ editor, document, unsaved }: { editor: DocumentEditor; document: DocumentView; unsaved: boolean }) {
  const [open, setOpen] = useState(false)
  const suggested = `${document.title} (copy)`.slice(0, 200)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Hint label="Save as copy">
        <DialogTrigger render={<Button size="icon" variant="ghost" aria-label="Save as copy" disabled={editor.busy} />}>
          <CopyPlusIcon aria-hidden="true" />
        </DialogTrigger>
      </Hint>
      <DialogContent>
        <form
          className="contents"
          onSubmit={(event) => {
            event.preventDefault()
            const title = new FormData(event.currentTarget).get('title')?.toString().trim()
            setOpen(false)
            void editor.saveAs(title || undefined)
          }}
        >
          <DialogHeader>
            <DialogTitle>Save as copy</DialogTitle>
            <DialogDescription>
              Saves what is on screen{unsaved ? ', including unsaved changes,' : ''} as a new document and opens it. “{document.title}” keeps its last save{unsaved ? ' and its unsaved changes move to the copy' : ''}.
            </DialogDescription>
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor="copy-title">Title of the copy</FieldLabel>
            <Input id="copy-title" name="title" defaultValue={suggested} maxLength={200} autoFocus />
          </Field>
          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit">Save copy</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function CloseButton({ editor, unsaved }: { editor: DocumentEditor; unsaved: boolean }) {
  // Controlled so confirming closes the dialog at once: if the discard is refused (another
  // client edited meanwhile), the conflict notice behind it must be reachable.
  const [confirming, setConfirming] = useState(false)
  const button = (
    <Button size="icon" variant="ghost" aria-label="Close document" onClick={unsaved ? undefined : () => void editor.close(false)}>
      <XIcon aria-hidden="true" />
    </Button>
  )
  if (!unsaved) return <Hint label="Close">{button}</Hint>
  return (
    <AlertDialog open={confirming} onOpenChange={setConfirming}>
      <AlertDialogTrigger render={button} />
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Discard unsaved changes?</AlertDialogTitle>
          <AlertDialogDescription>
            The last saved revision stays on disk. Changes since then will be lost.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep editing</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            onClick={() => {
              setConfirming(false)
              void editor.close(true)
            }}
          >
            Discard and close
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/** Charge steps are applied to the latest document, so quick repeated clicks all count. */
function changeCharge(atom: string, step: number) {
  return (document: DocumentView): Command[] | null => {
    const current = document.atoms.find((item) => item.id === atom)
    return current ? [{ op: 'update_atom', atom, charge: current.charge + step }] : null
  }
}

/** Shows the atom's element again after an empty entry or a rejected symbol. */
function ElementField({ atom, editor }: { atom: AtomView; editor: DocumentEditor }) {
  const [attempt, setAttempt] = useState(0)
  const reset = () => setAttempt((count) => count + 1)
  return (
    <Field>
      <FieldLabel htmlFor="atom-element">Element</FieldLabel>
      <Input
        id="atom-element"
        key={`${atom.element}:${attempt}`}
        defaultValue={atom.element}
        maxLength={3}
        onKeyDown={commitOnEnter}
        onBlur={(event) => {
          const value = event.currentTarget.value.trim()
          if (!value || value === atom.element) return reset()
          void editor.apply([{ op: 'update_atom', atom: atom.id, element: value }]).then(reset)
        }}
      />
    </Field>
  )
}

/** Empty text means natural abundance; anything but a mass number 1–999 is not sent. */
function parseIsotope(text: string): number | null | undefined {
  if (text === '') return null
  if (!/^\d{1,3}$/.test(text)) return undefined
  const value = Number(text)
  return value >= 1 ? value : undefined
}

function IsotopeField({ atom, editor }: { atom: AtomView; editor: DocumentEditor }) {
  const [invalid, setInvalid] = useState(false)
  return (
    <Field data-invalid={invalid || undefined}>
      <FieldLabel htmlFor="atom-isotope">Isotope mass number</FieldLabel>
      <Input
        id="atom-isotope"
        key={atom.isotope}
        inputMode="numeric"
        placeholder="Natural"
        defaultValue={atom.isotope ?? ''}
        aria-invalid={invalid || undefined}
        aria-describedby="atom-isotope-help"
        onKeyDown={commitOnEnter}
        onBlur={(event) => {
          const isotope = parseIsotope(event.currentTarget.value.trim())
          setInvalid(isotope === undefined)
          if (isotope !== undefined && isotope !== atom.isotope) {
            void editor.apply([{ op: 'update_atom', atom: atom.id, isotope }])
          }
        }}
      />
      {invalid ? (
        <FieldError id="atom-isotope-help">Enter a whole number from 1 to 999, or leave empty for natural abundance.</FieldError>
      ) : (
        <FieldDescription id="atom-isotope-help">Leave empty for natural abundance.</FieldDescription>
      )}
    </Field>
  )
}

function Inspector({
  document,
  selected,
  editor,
  onDelete,
}: {
  document: DocumentView
  selected: string[]
  editor: DocumentEditor
  onDelete: () => void
}) {
  const atom = selected.length === 1 ? document.atoms.find((item) => item.id === selected[0]) : undefined
  const bond = selected.length === 1 ? document.bonds.find((item) => item.id === selected[0]) : undefined

  return (
    <aside aria-label="Selection" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
      <h3 className="text-sm font-medium">
        {atom ? `Atom ${atom.id}` : bond ? `Bond ${bond.id}` : selected.length > 1 ? `${selected.length} selected` : 'Selection'}
      </h3>
      {selected.length === 0 && (
        <p className="text-xs leading-relaxed text-muted-foreground">
          Use the select tool (V) to inspect an atom or bond. {document.atoms.length} atoms, {document.bonds.length} bonds.
        </p>
      )}
      {atom && (
        <FieldGroup className="gap-3">
          <ElementField key={`element-${atom.id}`} atom={atom} editor={editor} />
          <Field>
            <FieldLabel id="atom-charge">Charge</FieldLabel>
            <div role="group" aria-labelledby="atom-charge" className="flex items-center gap-2">
              <Button
                size="icon-sm"
                variant="outline"
                aria-label="Decrease charge"
                onClick={() => void editor.apply(changeCharge(atom.id, -1))}
              >
                <MinusIcon aria-hidden="true" />
              </Button>
              <output className="min-w-8 text-center font-mono text-sm" aria-live="polite">
                {atom.charge > 0 ? `+${atom.charge}` : atom.charge}
              </output>
              <Button
                size="icon-sm"
                variant="outline"
                aria-label="Increase charge"
                onClick={() => void editor.apply(changeCharge(atom.id, 1))}
              >
                <PlusIcon aria-hidden="true" />
              </Button>
            </div>
          </Field>
          <IsotopeField key={`isotope-${atom.id}`} atom={atom} editor={editor} />
        </FieldGroup>
      )}
      {bond && (
        <Field>
          <FieldLabel id="bond-order">Order</FieldLabel>
          <ToggleGroup
            aria-labelledby="bond-order"
            variant="outline"
            size="sm"
            spacing={0}
            value={[bond.order]}
            onValueChange={(values) => {
              const order = values[0] as BondOrder | undefined
              if (order && order !== bond.order) void editor.apply([{ op: 'update_bond', bond: bond.id, order }])
            }}
          >
            {ORDERS.map((order) => (
              <ToggleGroupItem key={order.value} value={order.value} aria-label={order.label} className="min-w-8 font-mono">
                {order.glyph}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Field>
      )}
      {selected.length > 0 && (
        <Button size="sm" variant="outline" onClick={onDelete}>
          <Trash2Icon data-icon="inline-start" aria-hidden="true" />Delete
        </Button>
      )}
    </aside>
  )
}
