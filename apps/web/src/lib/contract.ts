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

export interface CapabilitiesResponse {
  api_version: typeof API_VERSION
  capabilities: Capability[]
}

export interface RuntimeSnapshot {
  health: HealthResponse
  capabilities: Capability[]
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
    new Set(value.capabilities.map((item) => item.id)).size === value.capabilities.length
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
    checkedAt: new Date(),
  }
}
