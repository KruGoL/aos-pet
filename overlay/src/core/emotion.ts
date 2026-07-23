// Pure mapping from capsule truth to what the pet shows. No DOM, no Pixi —
// this is the unit-tested heart of the overlay.

export interface Ailment { label: string }

export interface PetStatus {
  name?: string
  mood?: string
  level?: string
  frames?: string[]
  display?: string
  fullness?: number
  happiness?: number
  energy?: number
  cleanliness?: number
  ailments?: Ailment[]
  sleeping?: boolean
  message?: string
}

export type Presence =
  | { kind: 'ok'; status: PetStatus }
  | { kind: 'offline' }
  | { kind: 'no-pet'; message: string }
  | { kind: 'waking' }

export type Tint = 'ok' | 'resting' | 'warn' | 'critical' | 'offline'

export interface Emotion {
  frames: string[]
  bubble: string | null
  tint: Tint
  asleep: boolean
}

export const FALLBACK_FRAMES = ['(x_x)', '(x_x)  ']

const NEED_THRESHOLD = 30
const NEEDS: Array<{ key: keyof PetStatus; bubble: string }> = [
  { key: 'fullness', bubble: 'food?' },
  { key: 'cleanliness', bubble: 'soap?' },
  { key: 'energy', bubble: 'sleep?' },
  { key: 'happiness', bubble: 'play?' },
]

/** The capsule's frames are full status cards: art, then a blank line, then
 * name/bars/ailments. The overlay wants just the creature — everything
 * before the first blank line. */
export function artOnly(card: string): string {
  const lines = card.split('\n')
  const cut = lines.findIndex(l => l.trim() === '')
  return (cut === -1 ? lines : lines.slice(0, cut)).join('\n').trimEnd()
}

export function classify(payload: unknown, transportOk: boolean): Presence {
  if (!transportOk || payload == null || typeof payload !== 'object') return { kind: 'offline' }
  const obj = payload as Record<string, unknown>
  if (typeof obj.error === 'string') return { kind: 'no-pet', message: obj.error }
  return { kind: 'ok', status: obj as PetStatus }
}

function tintOf(level: string | undefined): Tint {
  return level === 'resting' || level === 'warn' || level === 'critical' ? level : 'ok'
}

export function emotionFor(p: Presence): Emotion {
  if (p.kind === 'offline')
    return { frames: FALLBACK_FRAMES, bubble: 'offline', tint: 'offline', asleep: true }
  if (p.kind === 'waking')
    return { frames: FALLBACK_FRAMES, bubble: '…', tint: 'ok', asleep: true }
  if (p.kind === 'no-pet')
    return { frames: FALLBACK_FRAMES, bubble: 'adopt me!', tint: 'warn', asleep: false }

  const s = p.status
  const cards = s.frames?.length ? s.frames : [s.display ?? '(?)']
  const frames = cards.map(c => artOnly(c) || c)
  const tint = tintOf(s.level)

  if (s.sleeping) return { frames, bubble: 'zzz', tint, asleep: true }
  if (s.ailments?.length)
    return { frames, bubble: s.ailments[0].label, tint: tint === 'ok' ? 'warn' : tint, asleep: false }

  let bubble: string | null = null
  let worst = NEED_THRESHOLD
  for (const need of NEEDS) {
    const value = s[need.key]
    if (typeof value === 'number' && value < worst) { worst = value; bubble = need.bubble }
  }
  return { frames, bubble, tint, asleep: false }
}
