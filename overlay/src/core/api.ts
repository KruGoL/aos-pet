import { classify, type Presence, type PetStatus } from './emotion'

const BASE = `http://127.0.0.1:${(import.meta as any).env?.VITE_PET_PORT ?? '8737'}`

async function json(path: string, init?: RequestInit): Promise<unknown> {
  const res = await fetch(BASE + path, { ...init, signal: AbortSignal.timeout(15_000) })
  if (res.status === 503) throw new Error('bridge: MCP down')
  return res.json()
}

export async function fetchStatus(): Promise<Presence> {
  try { return classify(await json('/status'), true) }
  catch { return classify(null, false) }
}

export async function postAction(tool: string, args?: object):
    Promise<{ message?: string; error?: string } & PetStatus> {
  const body = JSON.stringify({ tool, args: args ?? {} })
  return await json('/action', { method: 'POST', body,
    headers: { 'Content-Type': 'application/json' } }) as any
}

export async function fetchAlerts(): Promise<{ alerts?: { message: string }[] }> {
  return await json('/alerts') as any
}
