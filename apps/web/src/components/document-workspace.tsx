import { useEffect, useState, type KeyboardEvent, type ReactNode } from 'react'
import {
  ArchiveRestoreIcon,
  ArrowDownIcon,
  ArrowLeftIcon,
  ArrowRightIcon,
  ArrowUpIcon,
  AtomIcon,
  CopyIcon,
  CopyPlusIcon,
  EraserIcon,
  FilePlus2Icon,
  FolderOpenIcon,
  HexagonIcon,
  MinusIcon,
  MousePointer2Icon,
  PencilIcon,
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
import { Toggle } from '@/components/ui/toggle'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import * as edits from '@/editor/atom-edits'
import { stillApplicable } from '@/editor/commands'
import { queryDescription, queryLabel } from '@/editor/label'
import { deletePlacement, duplicatePlacement, movePlacement } from '@/editor/placement'
import { StructureCanvas, type DrawnOrder, type Tool } from '@/editor/structure-canvas'
import { DocumentStyleButton, PreferencesButton } from '@/components/style-settings'
import { PropertiesPanel } from '@/components/properties-panel'
import { AromaticityPanel } from '@/components/aromaticity-panel'
import type { AutosaveState } from '@/hooks/use-autosave'
import { useDocumentEditor, type CopiedFrom, type CopyNotice, type DocumentEditor } from '@/hooks/use-document-editor'
import { usePreferences } from '@/hooks/use-preferences'
import {
  ATTACHMENT_POINTS,
  sameInspectionSnapshot,
  type AtomView,
  type BondOrder,
  type BondView,
  type Command,
  type DocumentResponse,
  type DocumentSummary,
  type DocumentView,
  type GraphicShape,
  type InstanceView,
  type ListSort,
  type Position,
  type ReferenceTarget,
  type RecoveryView,
  type Hydrogens,
  type QueryAtom,
  type Radical,
  type StorageReason,
} from '@/lib/document-contract'

const ELEMENTS = ['C', 'H', 'N', 'O', 'S', 'P', 'F', 'Cl', 'Br', 'I']
const TOOLS: { value: Tool; label: string; key: string; icon: ReactNode }[] = [
  { value: 'select', label: 'Select and move', key: 'V', icon: <MousePointer2Icon /> },
  { value: 'atom', label: 'Atom', key: 'A', icon: <AtomIcon /> },
  { value: 'bond', label: 'Bond', key: 'B', icon: <SlashIcon /> },
  { value: 'erase', label: 'Erase', key: 'E', icon: <EraserIcon /> },
]
/** The bond tool records the selected kind without converting other bonds. */
const DRAWN_ORDERS: { value: DrawnOrder; label: string; glyph: string }[] = [
  { value: 'single', label: 'Single bond', glyph: '—' },
  { value: 'double', label: 'Double bond', glyph: '=' },
  { value: 'triple', label: 'Triple bond', glyph: '≡' },
  { value: 'aromatic', label: 'Aromatic bond', glyph: 'ar' },
  { value: 'dative', label: 'Dative bond, from where the drag starts (the donor)', glyph: '→' },
  { value: 'unknown', label: 'Bond of unknown kind', glyph: '~' },
]
/** The kinds a bond can be given in the inspector: a query bond starts as "any kind". */
const ORDERS: { value: BondOrder; label: string; glyph: string }[] = [
  ...DRAWN_ORDERS,
  { value: 'query', label: 'Query bond', glyph: '?' },
]
const QUERY_ATOMS: { value: Exclude<QueryAtom['kind'], 'list'>; label: string }[] = [
  { value: 'any', label: 'Query: any atom' },
  { value: 'any_heavy', label: 'Query: any atom except hydrogen' },
  { value: 'heteroatom', label: 'Query: any atom except carbon and hydrogen' },
  { value: 'halogen', label: 'Query: any halogen' },
]
const HYDROGEN_MODES: { value: Hydrogens['mode']; label: string; text: string }[] = [
  { value: 'inferred', label: 'Hydrogens inferred', text: 'Inferred' },
  { value: 'unknown', label: 'Hydrogens unknown', text: 'Unknown' },
  { value: 'specified', label: 'Hydrogens stated', text: 'Stated' },
]
const RADICALS: { value: Radical; label: string; glyph: string }[] = [
  { value: 'none', label: 'No radical', glyph: '0' },
  { value: 'singlet', label: 'Singlet radical', glyph: ':' },
  { value: 'doublet', label: 'Doublet radical', glyph: '•' },
  { value: 'triplet', label: 'Triplet radical', glyph: '∘∘' },
  { value: 'unknown', label: 'Radical unknown', glyph: '?' },
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

function ValenceInspector({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<{ snapshot: DocumentView; message: string } | null>(null)
  const [result, setResult] = useState<Awaited<ReturnType<DocumentEditor['inspectValence']>> | null>(null)
  const current = result && sameInspectionSnapshot(document, result.snapshot) ? result : null

  async function inspect() {
    const requested = document
    setPending(true)
    setError(null)
    try {
      setResult(await editor.inspectValence(requested.id))
    } catch (cause) {
      setResult(null)
      setError({ snapshot: requested, message: cause instanceof Error ? cause.message : 'Inspection failed.' })
    } finally {
      setPending(false)
    }
  }

  return <aside aria-label="Valence inspection" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
    <h3 className="text-sm font-medium">Valence inspection</h3>
    <p className="text-xs text-muted-foreground">Inspect valence and additional hydrogens without changing the drawing.</p>
    <Button size="sm" variant="outline" disabled={pending || editor.busy} onClick={() => void inspect()}>
      {pending ? 'Inspecting…' : 'Inspect valence'}
    </Button>
    {error && sameInspectionSnapshot(document, error.snapshot) && <p role="alert" className="text-xs">{error.message}</p>}
    {result && !current && <p className="text-xs">The inspected drawing differs from this one. Inspect again.</p>}
    {current && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto text-xs">
      <p>Model: {current.model}</p>
      {current.atoms.length === 0 && <p>No atoms to inspect.</p>}
      {current.atoms.map((atom) => <p key={atom.atom_id}>
        {atom.atom_id}: {atom.status}; hydrogens {atom.hydrogens ?? 'unknown'}
        {atom.reason && ` (${atom.reason.replaceAll('_', ' ')})`}
      </p>)}
    </div>}
  </aside>
}

/** Applies a text field's value on Enter or blur when it changed. */
function commitOnEnter(event: KeyboardEvent<HTMLInputElement>) {
  if (event.key === 'Enter') event.currentTarget.blur()
}

export function DocumentWorkspace() {
  const editor = useDocumentEditor()
  const preferences = usePreferences()
  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle><h2>Drawing workspace</h2></CardTitle>
        <CardDescription>
          {editor.current ? 'Edits apply on the local service; save to keep them.' : 'Create a document or open a saved one.'}
        </CardDescription>
        <CardAction className="flex items-center gap-2">
          <PreferencesButton preferences={preferences} current={editor.current?.document ?? null} editor={editor} />
          {editor.busy ? <Spinner /> : <Badge variant="outline">Local</Badge>}
        </CardAction>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-4">
        {editor.error && <ErrorNotice editor={editor} />}
        {editor.current ? <Editor editor={editor} current={editor.current} /> : <DocumentPicker editor={editor} />}
      </CardContent>
      <CardFooter className="gap-2">
        <p className="text-xs leading-relaxed text-muted-foreground">
          Atoms and bonds remain as drawn until edited. Valence and aromaticity use bounded models; stereo and naming remain unimplemented.
        </p>
      </CardFooter>
    </Card>
  )
}

/** Errors that report a state to act on rather than a failure. */
const INFORMATIONAL = new Set([
  'invalid_command',
  'save_unconfirmed',
  'request_in_progress',
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
    document_not_found: 'Document not found',
    unsaved_changes: 'Unsaved changes',
    save_conflict: 'Saved elsewhere',
    storage_failed: { read_only: 'Storage is read-only', busy: 'Document busy', io: 'Storage failed' }[error.reason ?? 'io'],
    save_unconfirmed: 'Save not confirmed',
    request_in_progress: 'Still in progress',
    draft_not_written: 'Left work not autosaved',
    invalid_command: 'Edit rejected',
    unreachable: 'Local service unavailable',
  }[error.code] ?? 'Request failed'
  return (
    <Alert variant={stale || INFORMATIONAL.has(error.code) ? 'default' : 'destructive'}>
      <TriangleAlertIcon aria-hidden="true" />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        {error.message}
        {error.stateReason && (
          <span className="block font-mono text-xs" data-testid="state-reason">
            Reason: {error.stateReason}
          </span>
        )}
      </AlertDescription>
      <AlertAction>
        {stale ? (
          <Button size="sm" variant="outline" onClick={() => void editor.reload()}>
            <RotateCwIcon data-icon="inline-start" aria-hidden="true" />Reload
          </Button>
        ) : error.code === 'save_unconfirmed' ? (
          <Button size="sm" variant="outline" onClick={() => void editor.save()}>Check again</Button>
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
        <ListControls editor={editor} />
        {list.kind === 'loading' && <Skeleton className="h-10 w-full" />}
        {list.kind === 'error' && <p className="text-sm text-muted-foreground">{list.message}</p>}
        {list.kind === 'ready' && list.list.documents.length === 0 && (
          <p className="text-sm text-muted-foreground">
            {editor.listQuery.query.trim() ? 'No saved document matches this search.' : 'Nothing saved yet.'}
          </p>
        )}
        {list.kind === 'ready' && list.list.documents.length > 0 && (
          <ul className="flex flex-col divide-y rounded-lg border">
            {list.list.documents.map((item) => (
              <li key={item.id} data-document-id={item.id} className="flex flex-wrap items-center justify-between gap-3 p-3">
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
                  <RenameButton editor={editor} item={item} />
                  <DeleteButton editor={editor} item={item} />
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
      <TrashSection editor={editor} />
    </div>
  )
}

/** How long typing pauses before the list is searched. */
const SEARCH_PAUSE_MS = 250

const SORTS: Array<{ value: ListSort; label: string }> = [
  { value: 'title', label: 'Title' },
  { value: 'recent', label: 'Recent' },
]

function ListControls({ editor }: { editor: DocumentEditor }) {
  const { listQuery, queryList } = editor
  const [text, setText] = useState(listQuery.query)
  // Searches once typing pauses, not on every keystroke.
  useEffect(() => {
    if (text === listQuery.query) return
    const timer = setTimeout(() => queryList({ ...listQuery, query: text }), SEARCH_PAUSE_MS)
    return () => clearTimeout(timer)
  }, [text, listQuery, queryList])
  return (
    <div className="flex flex-wrap items-end gap-3">
      <Field className="min-w-0 flex-1">
        <FieldLabel htmlFor="document-search">Search titles</FieldLabel>
        <Input
          id="document-search"
          type="search"
          value={text}
          maxLength={MAX_TITLE_CHARS}
          onChange={(event) => setText(event.currentTarget.value)}
        />
      </Field>
      <ToggleGroup
        aria-label="Sort documents"
        variant="outline"
        size="sm"
        value={[listQuery.sort]}
        onValueChange={(values) => {
          const sort = SORTS.find((item) => item.value === values[0])?.value
          if (sort) queryList({ ...listQuery, sort })
        }}
      >
        {SORTS.map((item) => (
          <ToggleGroupItem key={item.value} value={item.value}>{item.label}</ToggleGroupItem>
        ))}
      </ToggleGroup>
    </div>
  )
}

function RenameButton({ editor, item }: { editor: DocumentEditor; item: DocumentSummary }) {
  const [open, setOpen] = useState(false)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Hint label="Rename">
        <DialogTrigger render={<Button size="icon-sm" variant="ghost" aria-label={`Rename ${item.title}`} disabled={editor.busy} />}>
          <PencilIcon aria-hidden="true" />
        </DialogTrigger>
      </Hint>
      <DialogContent>
        <form
          className="contents"
          onSubmit={(event) => {
            event.preventDefault()
            const title = new FormData(event.currentTarget).get('title')?.toString().trim()
            setOpen(false)
            if (title && title !== item.title) void editor.rename(item.id, item.revision, title)
          }}
        >
          <DialogHeader>
            <DialogTitle>Rename document</DialogTitle>
            <DialogDescription>
              The new title is saved at once. A document with unsaved changes is renamed in the editor instead.
            </DialogDescription>
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor={`rename-${item.id}`}>Title</FieldLabel>
            <Input id={`rename-${item.id}`} name="title" defaultValue={item.title} maxLength={MAX_TITLE_CHARS} autoFocus />
          </Field>
          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit">Rename</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function DeleteButton({ editor, item }: { editor: DocumentEditor; item: DocumentSummary }) {
  // Controlled so confirming closes the dialog at once and a refusal shows behind it.
  const [confirming, setConfirming] = useState(false)
  return (
    <AlertDialog open={confirming} onOpenChange={setConfirming}>
      <Hint label="Move to trash">
        <AlertDialogTrigger render={<Button size="icon-sm" variant="ghost" aria-label={`Move ${item.title} to trash`} disabled={editor.busy} />}>
          <Trash2Icon aria-hidden="true" />
        </AlertDialogTrigger>
      </Hint>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Move “{item.title}” to the trash?</AlertDialogTitle>
          <AlertDialogDescription>
            Revision {item.revision} leaves Saved documents and can be restored from the trash. A document with unsaved
            changes or a recovery draft is not moved: save, discard, or recover that work first.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep it</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            onClick={() => {
              setConfirming(false)
              void editor.remove(item.id, item.revision)
            }}
          >
            Move to trash
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function TrashSection({ editor }: { editor: DocumentEditor }) {
  const { trash } = editor
  return (
    <section aria-labelledby="trash" className="flex flex-col gap-3">
      <h3 id="trash" className="text-sm font-medium">Trash</h3>
      {trash.kind === 'loading' && <Skeleton className="h-10 w-full" />}
      {trash.kind === 'error' && <p className="text-sm text-muted-foreground">{trash.message}</p>}
      {trash.kind === 'ready' && trash.trash.documents.length === 0 && (
        <p className="text-sm text-muted-foreground">The trash is empty.</p>
      )}
      {trash.kind === 'ready' && trash.trash.documents.length > 0 && (
        <ul className="flex flex-col divide-y rounded-lg border">
          {trash.trash.documents.map((item) => (
            <li key={item.id} data-trashed-id={item.id} className="flex flex-wrap items-center justify-between gap-3 p-3">
              <div className="flex min-w-0 flex-col gap-0.5">
                <span className="truncate text-sm font-medium">{item.title}</span>
                <span className="text-xs text-muted-foreground">
                  {item.atom_count} atoms · {item.bond_count} bonds · revision {item.revision}
                  {item.deleted_at_ms !== null && ` · deleted ${new Date(item.deleted_at_ms).toLocaleString()}`}
                </span>
              </div>
              <Button size="sm" variant="outline" onClick={() => void editor.restore(item.id)} disabled={editor.busy}>
                <ArchiveRestoreIcon data-icon="inline-start" aria-hidden="true" />Restore
              </Button>
            </li>
          ))}
        </ul>
      )}
      {trash.kind === 'ready' && trash.trash.unreadable.length > 0 && (
        <Alert variant="destructive">
          <TriangleAlertIcon aria-hidden="true" />
          <AlertTitle>Unreadable files in the trash</AlertTitle>
          <AlertDescription>
            {trash.trash.unreadable.map((item) => `${item.id}: ${item.message}`).join(' · ')}
          </AlertDescription>
        </Alert>
      )}
    </section>
  )
}

function Editor({ editor, current }: { editor: DocumentEditor; current: DocumentResponse }) {
  const { document, session } = current
  const [tool, setTool] = useState<Tool>('bond')
  const [element, setElement] = useState('C')
  const [bondOrder, setBondOrder] = useState<DrawnOrder>('single')
  const { selection, setSelection } = editor
  const exists = new Set([
    ...document.atoms.map((atom) => atom.id),
    ...document.bonds.map((bond) => bond.id),
    ...(document.scene?.objects?.map((object) => object.id) ?? []),
  ])
  const selected = new Set([...selection].filter((id) => exists.has(id)))
  const selectedIds = [...selected]

  function deleteSelection() {
    // Keep the IDs from this press while building against the document reached by the queue.
    const pressed = editor.selectionAtPress()
    void editor.apply((latest) => {
      const atoms = latest.atoms.filter((atom) => pressed.has(atom.id)).map((atom) => atom.id)
      const removed = new Set(atoms)
      const bonds = latest.bonds
        .filter((bond) => pressed.has(bond.id) && !removed.has(bond.begin) && !removed.has(bond.end))
        .map((bond) => bond.id)
      const objects = latest.scene?.objects?.filter((object) => pressed.has(object.id)).map((object) => object.id) ?? []
      const commands: Command[] = [
        // Removing selected owners first also removes their references to selected targets.
        ...(objects.length ? [{ op: 'delete_objects' as const, objects, mode: 'subtree' as const }] : []),
        ...(atoms.length ? [{ op: 'delete_atoms' as const, atoms }] : []),
        ...(bonds.length ? [{ op: 'delete_bonds' as const, bonds }] : []),
      ]
      return commands.length ? commands : null
    })
  }

  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, [contenteditable="true"]')) return
      // While a dialog is open the editor is inert, even before focus has moved into it.
      if (window.document.querySelector('[role="dialog"], [role="alertdialog"]')) return
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
        else if ((key === 'delete' || key === 'backspace') && editor.selectionAtPress().size > 0) {
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
            setBondOrder(values[0] as DrawnOrder)
            setTool('bond')
          }}
        >
          {DRAWN_ORDERS.map((order) => (
            <ToggleGroupItem key={order.value} value={order.value} aria-label={order.label} className="min-w-8 font-mono">
              {order.glyph}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        {document.settings && (
          <>
            <Separator orientation="vertical" className="h-6" />
            <DocumentStyleButton editor={editor} document={document} settings={document.settings} />
          </>
        )}
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
        <div className="flex min-w-0 flex-col gap-4">
          <Inspector document={document} selected={selectedIds} editor={editor} onDelete={deleteSelection} />
          <ValenceInspector document={document} editor={editor} />
          <PropertiesPanel document={document} editor={editor} />
          <AromaticityPanel document={document} editor={editor} />
          <SceneControls document={document} editor={editor} />
          {document.scene && <Placements instances={document.scene.instances} editor={editor} />}
        </div>
      </div>
      <p className="text-xs leading-relaxed text-muted-foreground">
        Bond tool: click an atom to grow a chain, drag to bond, click a bond to cycle its order. Atom tool: click empty space
        to place, click an atom to change its element. Select: drag atoms to move, drag empty space to box-select. Each
        placement draws the same atoms and bonds; edits to atoms and bonds apply to all, moves to the placement you drag in.
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

/** What the user can do when storage keeps refusing the draft, by the failure's reason. */
const STORAGE_ACTION: Record<StorageReason, string> = {
  read_only: 'Make the data directory writable; drafting resumes on the next retry.',
  busy: 'Another save holds the document; the next retry follows it.',
  io: 'Check the disk that holds the data directory.',
}

function draftFailure(state: Extract<AutosaveState, { kind: 'failed' }>) {
  const who = state.by === 'service' ? 'the service retries' : 'retrying'
  const times = state.attempts > 1 ? ` ${state.attempts} times` : ''
  const action = state.reason === null ? '' : ` ${STORAGE_ACTION[state.reason]}`
  return `failed${times}, ${who} at ${time(state.retryAt)}: ${state.message}${action}`
}

/** The recovery draft's state, apart from the saved file's. */
function DraftStatus({ state, onRetry }: { state: AutosaveState; onRetry: () => void }) {
  const text = {
    clean: 'not needed',
    waiting: 'pending…',
    saving: 'saving…',
    saved: state.kind === 'saved' ? `saved${state.at === null ? '' : ` ${time(state.at)}`} (revision ${state.revision})` : '',
    failed: state.kind === 'failed' ? draftFailure(state) : '',
    blocked:
      state.kind === 'blocked'
        ? state.recovery === 'in_use'
          ? 'paused while another process holds this document’s draft'
          : 'paused until the earlier unsaved work below is recovered or discarded'
        : '',
    stopped: state.kind === 'stopped' ? `stopped: ${state.message}` : '',
  }[state.kind]
  return (
    <span
      className="flex min-w-0 items-center gap-1.5"
      data-testid="draft-state"
      data-state={state.kind}
      data-by={state.kind === 'failed' ? state.by : undefined}
    >
      {state.kind === 'saving' && <Spinner className="size-3" />}
      <span className="min-w-0 break-words">Recovery draft: {text}</span>
      {state.kind === 'failed' && (
        <Button size="xs" variant="outline" onClick={onRetry}>Retry now</Button>
      )}
      {state.kind === 'blocked' && state.recovery === 'in_use' && (
        <Button size="xs" variant="outline" onClick={onRetry}>Check again</Button>
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
            <Button size="sm" onClick={() => void editor.recoverAsCopy()} disabled={unsaved || editor.busy}>Recover as copy</Button>
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

/** What the notice says became of the original when the copy was made. */
function copiedFromText(source: CopiedFrom, original: string) {
  switch (source.kind) {
    case 'recovered':
      return `The recovered work was saved as this new document. ${original} is unchanged.`
    case 'closed':
      return `This is a new, separate document. ${original} keeps its last save${source.discardedChanges ? '; the changes that were unsaved there are only in this copy' : ''}.`
    case 'left_open':
      return `This is a new, separate document. ${original} was left open with its unsaved changes, which this copy also has, because its recovery draft could not be removed. Open the original to save or discard them.`
    default: {
      const unhandled: never = source
      return unhandled
    }
  }
}

function CopyNoticeBanner({ editor, notice, title }: { editor: DocumentEditor; notice: CopyNotice; title: string }) {
  return (
    <Alert data-testid="copy-notice" data-source={notice.source.kind}>
      <CopyPlusIcon aria-hidden="true" />
      <AlertTitle>Now editing the copy “{title}”</AlertTitle>
      <AlertDescription>{copiedFromText(notice.source, `“${notice.sourceTitle}”`)}</AlertDescription>
      <AlertAction>
        <Button size="sm" variant="outline" onClick={() => void editor.open(notice.sourceId)} disabled={editor.busy}>
          Open original
        </Button>
      </AlertAction>
    </Alert>
  )
}

/** The service's limit, in characters (code points) rather than UTF-16 units. */
const MAX_TITLE_CHARS = 200

/** The title the service would give the copy: cut by characters, never mid-pair, to fit the suffix. */
function copyTitle(title: string) {
  const suffix = ' (copy)'
  return Array.from(title).slice(0, MAX_TITLE_CHARS - suffix.length).join('').trimEnd() + suffix
}

function SaveAsButton({ editor, document, unsaved }: { editor: DocumentEditor; document: DocumentView; unsaved: boolean }) {
  const [open, setOpen] = useState(false)
  const suggested = copyTitle(document.title)
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

/**
 * Shows the atom's element again after an empty entry or a rejected symbol. A query atom shows
 * no element; entering one makes it an element atom.
 */
function ElementField({ atom, editor }: { atom: AtomView; editor: DocumentEditor }) {
  const [attempt, setAttempt] = useState(0)
  const reset = () => setAttempt((count) => count + 1)
  const symbol = atom.kind === 'element' ? atom.element : ''
  return (
    <Field>
      <FieldLabel htmlFor="atom-element">Element</FieldLabel>
      <Input
        id="atom-element"
        key={`${atom.kind}:${symbol}:${attempt}`}
        defaultValue={symbol}
        placeholder={atom.kind === 'query' ? 'Symbol' : undefined}
        maxLength={3}
        onKeyDown={commitOnEnter}
        onBlur={(event) => {
          const value = event.currentTarget.value.trim()
          if (!value || value === symbol) return reset()
          void editor.apply(edits.setElement(atom.id, value)).then(reset)
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
        disabled={atom.kind === 'query'}
        inputMode="numeric"
        placeholder="Natural"
        defaultValue={atom.isotope ?? ''}
        aria-invalid={invalid || undefined}
        aria-describedby="atom-isotope-help"
        onKeyDown={commitOnEnter}
        onBlur={(event) => {
          const isotope = parseIsotope(event.currentTarget.value.trim())
          setInvalid(isotope === undefined)
          if (isotope !== undefined && isotope !== atom.isotope) void editor.apply(edits.setIsotope(atom.id, isotope))
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

/** A single command as a transaction, or nothing to send. */
const commandList = (command: Command | null) => (command ? [command] : null)

/** How far one press of a move button takes a placement, in points. */
const NUDGE = 10
const NUDGES: { label: string; offset: Position; icon: ReactNode }[] = [
  { label: 'left', offset: { x: -NUDGE, y: 0 }, icon: <ArrowLeftIcon /> },
  { label: 'up', offset: { x: 0, y: -NUDGE }, icon: <ArrowUpIcon /> },
  { label: 'down', offset: { x: 0, y: NUDGE }, icon: <ArrowDownIcon /> },
  { label: 'right', offset: { x: NUDGE, y: 0 }, icon: <ArrowRightIcon /> },
]

/** Scene selection stays interactive while an edit is in flight. */
function SceneControls({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const objects = document.scene?.objects
  const roots = document.scene?.roots
  const references = document.scene?.references
  if (!objects || !roots || !references) return null
  const chosen = objects.filter((object) => editor.selection.has(object.id))
  const groupable = chosen.length >= 2 && chosen.every((object) => object.parent === chosen[0].parent)
  const one = chosen.length === 1 ? chosen[0] : undefined
  const siblings = one?.parent === null ? roots : objects.find((object) => object.id === one?.parent && object.kind === 'group')
  const siblingsOfOne = Array.isArray(siblings) ? siblings : siblings?.kind === 'group' ? siblings.children : []
  const index = one ? siblingsOfOne.indexOf(one.id) : -1
  const graphTargets: ReferenceTarget[] = [
    ...document.atoms.filter((atom) => editor.selection.has(atom.id)).map((atom): ReferenceTarget => ({ kind: 'atom', id: atom.id })),
    ...document.bonds.filter((bond) => editor.selection.has(bond.id)).map((bond): ReferenceTarget => ({ kind: 'bond', id: bond.id })),
  ]
  const link = chosen.length === 1 && graphTargets.length === 1
    ? { source: chosen[0].id, target: graphTargets[0] }
    : chosen.length === 2 && graphTargets.length === 0
      ? { source: chosen[0].id, target: { kind: 'object' as const, id: chosen[1].id } }
      : undefined
  const alreadyLinked = link && references.some((reference) =>
    reference.source === link.source && reference.target.kind === link.target.kind && reference.target.id === link.target.id,
  )

  function add(shape: GraphicShape) {
    void editor.apply((latest) => {
      const count = latest.scene?.objects?.filter((object) => object.kind === 'graphic').length ?? 0
      const x = 20 + (count % 6) * 35
      const y = 20 + Math.floor(count / 6) * 30
      return [{ op: 'add_graphic', shape, points: [{ x, y }, { x: x + 24, y: y + (shape === 'rectangle' ? 16 : 0) }] }]
    })
  }

  function reorder(id: string, step: number) {
    void editor.apply((latest) => {
      const object = latest.scene?.objects?.find((item) => item.id === id)
      if (!object) return null
      const parent = object.parent === null ? undefined : latest.scene?.objects?.find((item) => item.id === object.parent)
      const list = object.parent === null ? latest.scene?.roots : parent?.kind === 'group' ? parent.children : undefined
      if (!list) return null
      const target = list.indexOf(id) + step
      return target >= 0 && target < list.length ? [{ op: 'reorder', object: id, index: target }] : null
    })
  }

  return (
    <section aria-labelledby="scene-heading" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
      <h3 id="scene-heading" className="text-sm font-medium">Scene</h3>
      <ul className="flex flex-col gap-1">
        {objects.map((object) => (
          <li key={object.id}>
            <Toggle
              size="sm"
              variant="outline"
              aria-label={`Select scene object ${object.id}`}
              pressed={editor.selection.has(object.id)}
              onPressedChange={(pressed) => {
                editor.setSelection((current) => {
                  const next = new Set(current)
                  if (pressed) next.add(object.id)
                  else next.delete(object.id)
                  return next
                })
              }}
            >
              {object.id} {object.kind === 'graphic' ? object.shape : object.kind}
              {object.parent ? ` in ${object.parent}` : ''}
            </Toggle>
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap gap-1">
        {(['line', 'rectangle', 'arrow'] as const).map((shape) => (
          <Button key={shape} size="sm" variant="outline" onClick={() => add(shape)}>Add {shape}</Button>
        ))}
      </div>
      <Button size="sm" variant="outline" disabled={!link || Boolean(alreadyLinked)} onClick={() => {
        if (link) void editor.apply((latest) => stillApplicable([{ op: 'add_reference', ...link }], latest))
      }}>Add reference</Button>
      {references.length > 0 && (
        <ul aria-label="Scene references" className="flex flex-col gap-1">
          {references.map((reference) => (
            <li key={`${reference.source}:${reference.target.kind}:${reference.target.id}`} className="flex items-center gap-1">
              <span className="text-xs">{reference.source} → {reference.target.kind} {reference.target.id}</span>
              <Button size="icon-xs" variant="ghost" aria-label={`Remove reference from ${reference.source} to ${reference.target.kind} ${reference.target.id}`}
                onClick={() => void editor.apply((latest) => stillApplicable([{ op: 'remove_reference', ...reference }], latest))}>
                <XIcon aria-hidden="true" />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap gap-1">
        <Button size="sm" variant="outline" disabled={!groupable} onClick={() => {
          const ids = chosen.map((object) => object.id)
          void editor.apply((latest) => {
            const selected = latest.scene?.objects?.filter((object) => ids.includes(object.id)) ?? []
            return selected.length === ids.length && selected.every((object) => object.parent === selected[0].parent)
              ? [{ op: 'group', objects: ids }]
              : null
          }, { kind: 'group', objects: ids })
        }}>Group</Button>
        <Button size="sm" variant="outline" disabled={one?.kind !== 'group'} onClick={() => {
          if (one?.kind === 'group') void editor.apply((latest) => stillApplicable([{ op: 'ungroup', group: one.id }], latest))
        }}>Ungroup</Button>
        <Button size="sm" variant="outline" disabled={!one || index <= 0} onClick={() => one && reorder(one.id, -1)}>Earlier</Button>
        <Button size="sm" variant="outline" disabled={!one || index < 0 || index === siblingsOfOne.length - 1} onClick={() => one && reorder(one.id, 1)}>Later</Button>
      </div>
      <div className="flex flex-wrap gap-1">
        {NUDGES.map((nudge) => (
          <Button key={nudge.label} size="icon-xs" variant="outline" aria-label={`Move selected scene objects ${nudge.label}`}
            disabled={chosen.length === 0} onClick={() => {
              const ids = chosen.map((object) => object.id)
              void editor.apply((latest) => stillApplicable([{ op: 'move_objects', objects: ids, delta: nudge.offset }], latest))
            }}>
            {nudge.icon}
          </Button>
        ))}
      </div>
    </section>
  )
}

/**
 * The document's scene instances: each draws the same graph at its own positions. A placement
 * is duplicated, moved, or deleted as a whole; the last one cannot be deleted.
 */
function Placements({ instances, editor }: { instances: InstanceView[]; editor: DocumentEditor }) {
  // Each command is built from the latest document when its request runs, after any queued
  // before it: a copy is made of the placement that is last then, and quick presses all count.
  const duplicate = () => void editor.apply((latest) => commandList(duplicatePlacement(latest)))
  return (
    <section aria-labelledby="placements-heading" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
      <h3 id="placements-heading" className="text-sm font-medium">Placements</h3>
      <ul className="flex flex-col gap-2">
        {instances.map((instance) => (
          <li key={instance.id} data-placement={instance.id} className="flex flex-wrap items-center gap-1">
            <span className="mr-auto font-mono text-xs">{instance.id}</span>
            {NUDGES.map((nudge) => (
              <Button
                key={nudge.label}
                size="icon-xs"
                variant="outline"
                aria-label={`Move placement ${instance.id} ${nudge.label}`}
                onClick={() => void editor.apply((latest) => commandList(movePlacement(latest, instance.id, nudge.offset)))}
              >
                {nudge.icon}
              </Button>
            ))}
            <Button
              size="icon-xs"
              variant="ghost"
              aria-label={`Delete placement ${instance.id}`}
              disabled={instances.length === 1}
              onClick={() => void editor.apply((latest) => commandList(deletePlacement(latest, instance.id)))}
            >
              <Trash2Icon aria-hidden="true" />
            </Button>
          </li>
        ))}
      </ul>
      <Button
        size="sm"
        variant="outline"
        onClick={duplicate}
      >
        <CopyIcon data-icon="inline-start" aria-hidden="true" />Duplicate placement
      </Button>
    </section>
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
  const object = selected.length === 1 ? document.scene?.objects?.find((item) => item.id === selected[0]) : undefined

  return (
    <aside aria-label="Selection" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
      <h3 className="text-sm font-medium">
        {atom ? `Atom ${atom.id}` : bond ? `Bond ${bond.id}` : object ? `Scene object ${object.id}` : selected.length > 1 ? `${selected.length} selected` : 'Selection'}
      </h3>
      {selected.length === 0 && (
        <p className="text-xs leading-relaxed text-muted-foreground">
          Use the select tool (V) to inspect an atom or bond. {document.atoms.length} atoms, {document.bonds.length} bonds.
        </p>
      )}
      {atom && <AtomInspector atom={atom} editor={editor} />}
      {bond && <BondInspector bond={bond} editor={editor} />}
      {selected.length > 0 && (
        <Button size="sm" variant="outline" onClick={onDelete}>
          <Trash2Icon data-icon="inline-start" aria-hidden="true" />Delete
        </Button>
      )}
    </aside>
  )
}

/** A row of small labelled controls that wraps inside the narrow inspector. */
function Choice({ label, id, children }: { label: string; id: string; children: ReactNode }) {
  return (
    <Field>
      <FieldLabel id={id}>{label}</FieldLabel>
      {children}
    </Field>
  )
}

/** A value with buttons that step it; a step that no longer applies is not sent. */
function Stepper({
  name,
  value,
  onStep,
  disabled,
}: {
  name: string
  value: string
  onStep: (step: number) => void
  disabled?: boolean
}) {
  return (
    <div className="flex items-center gap-2">
      <Button size="icon-sm" variant="outline" aria-label={`Decrease ${name}`} disabled={disabled} onClick={() => onStep(-1)}>
        <MinusIcon aria-hidden="true" />
      </Button>
      <output className="min-w-8 text-center font-mono text-sm" aria-live="polite">
        {value}
      </output>
      <Button size="icon-sm" variant="outline" aria-label={`Increase ${name}`} disabled={disabled} onClick={() => onStep(1)}>
        <PlusIcon aria-hidden="true" />
      </Button>
    </div>
  )
}

/**
 * An atom's element or query, charge, isotope, and recorded states. Every change is built from
 * the latest document when its request runs (see editor/atom-edits), so quick presses in any
 * order never send a combination the service refuses.
 */
function AtomInspector({ atom, editor }: { atom: AtomView; editor: DocumentEditor }) {
  const query = atom.kind === 'query' ? atom.query : null
  return (
    <FieldGroup className="gap-3">
      <ElementField key={`element-${atom.id}`} atom={atom} editor={editor} />
      <Choice label="Query atom" id="atom-query">
        <ToggleGroup
          aria-labelledby="atom-query"
          variant="outline"
          size="sm"
          spacing={0}
          value={query && query.kind !== 'list' ? [query.kind] : []}
          onValueChange={(values) => {
            const kind = QUERY_ATOMS.find((item) => item.value === values[0])?.value
            if (kind) void editor.apply(edits.setQuery(atom.id, { kind }))
          }}
        >
          {QUERY_ATOMS.map((item) => (
            <ToggleGroupItem key={item.value} value={item.value} aria-label={item.label} className="min-w-8 font-mono">
              {queryLabel({ kind: item.value })}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </Choice>
      <QueryListField key={`list-${atom.id}-${JSON.stringify(query)}`} atom={atom} editor={editor} />
      {query && (
        <p className="text-xs leading-relaxed text-muted-foreground" data-testid="query-expression">
          Query {queryLabel(query)}: {queryDescription(query)}. Recorded only; nothing is matched.
        </p>
      )}
      <Field data-disabled={query ? true : undefined}>
        <FieldLabel id="atom-charge">Charge</FieldLabel>
        <div role="group" aria-labelledby="atom-charge">
          <Stepper
            name="charge"
            value={atom.charge > 0 ? `+${atom.charge}` : String(atom.charge)}
            disabled={query !== null}
            onStep={(step) => void editor.apply(edits.stepCharge(atom.id, step))}
          />
        </div>
      </Field>
      <IsotopeField key={`isotope-${atom.id}`} atom={atom} editor={editor} />
      <Choice label="Hydrogens" id="atom-hydrogens">
        <ToggleGroup
          aria-labelledby="atom-hydrogens"
          variant="outline"
          size="sm"
          spacing={0}
          value={[atom.hydrogens.mode]}
          onValueChange={(values) => {
            const mode = HYDROGEN_MODES.find((item) => item.value === values[0])?.value
            if (mode) void editor.apply(edits.setHydrogenMode(atom.id, mode))
          }}
        >
          {HYDROGEN_MODES.map((item) => (
            <ToggleGroupItem key={item.value} value={item.value} aria-label={item.label} className="text-xs">
              {item.text}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        {atom.hydrogens.mode === 'specified' && (
          <Stepper
            name="stated hydrogens"
            value={`H${atom.hydrogens.count}`}
            onStep={(step) => void editor.apply(edits.stepHydrogens(atom.id, step))}
          />
        )}
      </Choice>
      <Choice label="Radical" id="atom-radical">
        <ToggleGroup
          aria-labelledby="atom-radical"
          variant="outline"
          size="sm"
          spacing={0}
          value={[atom.radical]}
          onValueChange={(values) => {
            const radical = RADICALS.find((item) => item.value === values[0])?.value
            if (radical) void editor.apply(edits.setRadical(atom.id, radical))
          }}
        >
          {RADICALS.map((item) => (
            <ToggleGroupItem key={item.value} value={item.value} aria-label={item.label} className="min-w-8 font-mono">
              {item.glyph}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </Choice>
      <Choice label="Attachment points" id="atom-attachment">
        <div role="group" aria-labelledby="atom-attachment" className="flex gap-1">
          {ATTACHMENT_POINTS.map((point) => (
            <Toggle
              key={point}
              variant="outline"
              size="sm"
              className="min-w-8 font-mono"
              aria-label={`Attachment point ${point}`}
              pressed={atom.attachment_points.includes(point)}
              onPressedChange={() => void editor.apply(edits.toggleAttachmentPoint(atom.id, point))}
            >
              {point}
            </Toggle>
          ))}
        </div>
      </Choice>
    </FieldGroup>
  )
}

/**
 * A query atom's element list, typed as "N, O, S", with whether it is negated. Each change is
 * built from the latest list and negation, so a list typed while a negation is on its way (or
 * the reverse) keeps both.
 */
function QueryListField({ atom, editor }: { atom: AtomView; editor: DocumentEditor }) {
  const list = atom.query?.kind === 'list' ? atom.query : null
  const [invalid, setInvalid] = useState(false)
  const negated = list?.negated ?? false
  return (
    <Field data-invalid={invalid || undefined}>
      <FieldLabel htmlFor="atom-query-list">Query element list</FieldLabel>
      <div className="flex items-center gap-1">
        <Input
          id="atom-query-list"
          placeholder="N, O, S"
          defaultValue={list?.elements.join(', ') ?? ''}
          aria-invalid={invalid || undefined}
          aria-describedby="atom-query-list-help"
          onKeyDown={commitOnEnter}
          onBlur={(event) => {
            const text = event.currentTarget.value.trim()
            if (text === (list?.elements.join(', ') ?? '')) return setInvalid(false)
            const elements = edits.parseQueryList(text)
            setInvalid(elements === null)
            if (elements) void editor.apply(edits.setQueryElements(atom.id, elements))
          }}
        />
        <Toggle
          variant="outline"
          size="sm"
          aria-label="Negate the list: any atom except these"
          disabled={!list}
          pressed={negated}
          onPressedChange={(pressed) => void editor.apply(edits.setQueryNegated(atom.id, pressed))}
        >
          not
        </Toggle>
      </div>
      {invalid ? (
        <FieldError id="atom-query-list-help">Enter 1 to 10 different element symbols.</FieldError>
      ) : (
        <FieldDescription id="atom-query-list-help">Up to 10 elements; the canvas shows the first 3.</FieldDescription>
      )}
    </Field>
  )
}

/** A bond's kind, and for a query bond the kinds it allows (none listed: any kind). */
function BondInspector({ bond, editor }: { bond: BondView; editor: DocumentEditor }) {
  const listed = bond.query?.kind === 'list' ? bond.query.orders : []
  return (
    <FieldGroup className="gap-3">
      <Choice label="Kind" id="bond-order">
        <ToggleGroup
          aria-labelledby="bond-order"
          variant="outline"
          size="sm"
          spacing={1}
          className="flex-wrap"
          value={[bond.order]}
          onValueChange={(values) => {
            const order = ORDERS.find((item) => item.value === values[0])?.value
            if (!order) return
            const kind = order === 'query' ? { order, query: { kind: 'any' as const } } : { order }
            void editor.apply(edits.setBondKind(bond.id, kind))
          }}
        >
          {ORDERS.map((order) => (
            <ToggleGroupItem key={order.value} value={order.value} aria-label={order.label} className="min-w-8 font-mono">
              {order.glyph}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </Choice>
      {bond.order === 'dative' && (
        <p className="text-xs text-muted-foreground" data-testid="dative-direction">
          From {bond.begin} (the donor) to {bond.end}. To reverse it, draw it again from {bond.end}.
        </p>
      )}
      {bond.order === 'query' && (
        <Choice label="Query bond allows" id="bond-query">
          <div role="group" aria-labelledby="bond-query" className="flex flex-wrap gap-1">
            {edits.QUERY_BOND_ORDERS.map((order) => (
              <Toggle
                key={order}
                variant="outline"
                size="sm"
                className="text-xs"
                aria-label={`Query bond allows ${order}`}
                pressed={listed.includes(order)}
                onPressedChange={() => void editor.apply(edits.toggleQueryBondOrder(bond.id, order))}
              >
                {order}
              </Toggle>
            ))}
          </div>
          <p className="text-xs text-muted-foreground" data-testid="bond-query-expression">
            {listed.length > 0 ? `One of: ${listed.join(', ')}.` : 'Any kind.'} Recorded only; nothing is matched.
          </p>
        </Choice>
      )}
    </FieldGroup>
  )
}
