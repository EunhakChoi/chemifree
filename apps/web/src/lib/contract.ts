/** Hand-maintained HTTP v1 mirror. Rust DTOs live in crates/application/src/lib.rs. */
export const API_VERSION = 'v1' as const

export interface HealthResponse {
  api_version: typeof API_VERSION
  service: string
  version: string
  stage: 'scaffold'
  status: 'ok'
}

export interface Capability {
  id: string
  label: string
  status: 'available' | 'not_implemented'
  description: string
}

/** Whether a document and the operation contract can hold an operation's inputs and results. */
export type Representation = 'supported' | 'not_supported'

/** Whether the service runs an operation. Only `executable` means it does. */
export type Execution = 'executable' | 'requires_data' | 'not_implemented' | 'rejected'

export interface OperationSupport {
  id: string
  representation: Representation
  execution: Execution
  /** The plan entry expected to change this, for an operation that does not run. */
  followup?: string
  note: string
  /**
   * On a sub-operation that is a capability rather than a command: the commands that carry it
   * out (empty while it is not implemented). Absent on command rows and in older replies.
   */
  commands?: string[]
  sub_operations?: OperationSupport[]
}

export interface ProfileSupport {
  id: string
  execution: Execution
  note: string
}

/** The operation contract's catalog (`capabilities.inspect`), mirrored in the capabilities reply. */
export interface OperationCatalog {
  implementation: string
  version: string
  contracts: number[]
  profiles: ProfileSupport[]
  operations: OperationSupport[]
}

export interface CapabilitiesResponse {
  api_version: typeof API_VERSION
  capabilities: Capability[]
  /** Added within v1; a service from before it leaves it out. */
  operation_catalog?: OperationCatalog
}

export interface RuntimeSnapshot {
  health: HealthResponse
  capabilities: Capability[]
  /** Null when the service predates the operation catalog. */
  operationCatalog: OperationCatalog | null
  checkedAt: Date
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isCapability(value: unknown): value is Capability {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.label === 'string' &&
    typeof value.description === 'string' &&
    (value.status === 'available' || value.status === 'not_implemented')
  )
}

const REPRESENTATIONS: readonly unknown[] = ['supported', 'not_supported']
const EXECUTIONS: readonly unknown[] = ['executable', 'requires_data', 'not_implemented', 'rejected']

function hasDistinctIds(items: Array<{ id: string }>): boolean {
  return new Set(items.map((item) => item.id)).size === items.length
}

function isOperationSupport(value: unknown): value is OperationSupport {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    REPRESENTATIONS.includes(value.representation) &&
    EXECUTIONS.includes(value.execution) &&
    typeof value.note === 'string' &&
    (value.followup === undefined || typeof value.followup === 'string') &&
    (value.commands === undefined ||
      (Array.isArray(value.commands) && value.commands.every((command) => typeof command === 'string'))) &&
    (value.sub_operations === undefined ||
      (Array.isArray(value.sub_operations) &&
        value.sub_operations.every(isOperationSupport) &&
        hasDistinctIds(value.sub_operations)))
  )
}

function isProfileSupport(value: unknown): value is ProfileSupport {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    EXECUTIONS.includes(value.execution) &&
    typeof value.note === 'string'
  )
}

function isOperationCatalog(value: unknown): value is OperationCatalog {
  return (
    isRecord(value) &&
    typeof value.implementation === 'string' &&
    typeof value.version === 'string' &&
    Array.isArray(value.contracts) &&
    value.contracts.length > 0 &&
    value.contracts.every((contract) => Number.isInteger(contract)) &&
    Array.isArray(value.profiles) &&
    value.profiles.every(isProfileSupport) &&
    hasDistinctIds(value.profiles) &&
    Array.isArray(value.operations) &&
    value.operations.length > 0 &&
    value.operations.every(isOperationSupport) &&
    hasDistinctIds(value.operations)
  )
}

function isHealth(value: unknown): value is HealthResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    typeof value.service === 'string' &&
    typeof value.version === 'string' &&
    value.stage === 'scaffold' &&
    value.status === 'ok'
  )
}

function isCapabilities(value: unknown): value is CapabilitiesResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    Array.isArray(value.capabilities) &&
    value.capabilities.length > 0 &&
    value.capabilities.every(isCapability) &&
    hasDistinctIds(value.capabilities) &&
    (value.operation_catalog === undefined || isOperationCatalog(value.operation_catalog))
  )
}

async function readJson(path: string, signal: AbortSignal): Promise<unknown> {
  const response = await fetch(path, { signal, headers: { Accept: 'application/json' } })
  if (!response.ok) throw new Error(`The local service returned HTTP ${response.status}.`)
  return response.json() as Promise<unknown>
}

export async function readRuntime(signal: AbortSignal): Promise<RuntimeSnapshot> {
  const requestSignal = AbortSignal.any([signal, AbortSignal.timeout(5_000)])
  const [health, response] = await Promise.all([
    readJson('/api/v1/health', requestSignal),
    readJson('/api/v1/capabilities', requestSignal),
  ])

  if (!isHealth(health) || !isCapabilities(response)) {
    throw new Error('The local service returned an incompatible API v1 response.')
  }

  return {
    health,
    capabilities: response.capabilities,
    operationCatalog: response.operation_catalog ?? null,
    checkedAt: new Date(),
  }
}
