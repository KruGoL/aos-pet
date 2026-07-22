import { describe, expect, it } from 'vitest'
import { classify, emotionFor, FALLBACK_FRAMES, type PetStatus } from './emotion'

const base: PetStatus = {
  name: 'Rex', mood: 'content', level: 'ok', frames: ['(o.o)', '(-.-)'],
  fullness: 80, happiness: 80, energy: 80, cleanliness: 80, ailments: [], sleeping: false,
}

const ok = (over: Partial<PetStatus>) => ({ kind: 'ok' as const, status: { ...base, ...over } })

describe('classify', () => {
  it('transport down -> offline', () => {
    expect(classify(null, false)).toEqual({ kind: 'offline' })
  })
  it('domain error -> no-pet with message', () => {
    const p = classify({ error: 'No pet yet — adopt one' }, true)
    expect(p.kind).toBe('no-pet')
  })
  it('normal payload -> ok', () => {
    expect(classify(base, true)).toEqual({ kind: 'ok', status: base })
  })
})

describe('emotionFor', () => {
  it('healthy pet: no bubble, capsule frames', () => {
    const e = emotionFor(ok({}))
    expect(e.bubble).toBeNull()
    expect(e.frames).toEqual(base.frames)
    expect(e.tint).toBe('ok')
  })
  it('hunger under threshold -> еда?', () => {
    expect(emotionFor(ok({ fullness: 20 })).bubble).toBe('еда?')
  })
  it('lowest need wins', () => {
    expect(emotionFor(ok({ fullness: 25, cleanliness: 10 })).bubble).toBe('мыло?')
  })
  it('energy -> сон?, happiness -> мяч?', () => {
    expect(emotionFor(ok({ energy: 5 })).bubble).toBe('сон?')
    expect(emotionFor(ok({ happiness: 5 })).bubble).toBe('мяч?')
  })
  it('ailment label beats needs', () => {
    const e = emotionFor(ok({ fullness: 1, ailments: [{ label: 'tummy ache' }] }))
    expect(e.bubble).toBe('tummy ache')
    expect(e.tint).toBe('warn')
  })
  it('sleeping beats needs and freezes walker', () => {
    const e = emotionFor(ok({ fullness: 1, sleeping: true }))
    expect(e.bubble).toBe('zzz')
    expect(e.asleep).toBe(true)
  })
  it('critical level propagates as tint', () => {
    expect(emotionFor(ok({ level: 'critical' })).tint).toBe('critical')
  })
  it('offline: grey, fallback frames, asleep pose', () => {
    const e = emotionFor({ kind: 'offline' })
    expect(e).toEqual({ frames: FALLBACK_FRAMES, bubble: 'offline', tint: 'offline', asleep: true })
  })
  it('waking: hourglass bubble', () => {
    expect(emotionFor({ kind: 'waking' }).bubble).toBe('…')
  })
  it('no-pet: bubble asks to adopt', () => {
    expect(emotionFor({ kind: 'no-pet', message: 'x' }).bubble).toBe('заведи меня!')
  })
})
