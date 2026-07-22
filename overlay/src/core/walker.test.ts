import { describe, expect, it } from 'vitest'
import { BASE_SPEED_PX_S, createWalker, tickWalker } from './walker'

const bounds = { min: 0, max: 500 }
const rWalk = () => 0.99 // rand high -> pick "walk" period

describe('walker', () => {
  it('walks in its direction', () => {
    const s = { ...createWalker(100), mode: 'walk' as const, dir: 1 as const, remainingMs: 10_000 }
    const next = tickWalker(s, 1000, bounds, 100, false, rWalk)
    expect(next.x).toBeCloseTo(100 + BASE_SPEED_PX_S)
  })
  it('flips at the right bound', () => {
    const s = { ...createWalker(499), mode: 'walk' as const, dir: 1 as const, remainingMs: 10_000 }
    const next = tickWalker(s, 1000, bounds, 100, false, rWalk)
    expect(next.dir).toBe(-1)
    expect(next.x).toBeLessThanOrEqual(500)
  })
  it('asleep -> frozen', () => {
    const s = { ...createWalker(100), mode: 'walk' as const, remainingMs: 5000 }
    expect(tickWalker(s, 1000, bounds, 100, true, rWalk).x).toBe(100)
  })
  it('low energy is slower than high energy', () => {
    const s = { ...createWalker(100), mode: 'walk' as const, dir: 1 as const, remainingMs: 10_000 }
    const slow = tickWalker(s, 1000, bounds, 0, false, rWalk).x
    const fast = tickWalker(s, 1000, bounds, 100, false, rWalk).x
    expect(slow).toBeLessThan(fast)
    expect(slow).toBeGreaterThan(100) // never fully stops while awake
  })
  it('pause does not move', () => {
    const s = { ...createWalker(100), mode: 'pause' as const, remainingMs: 5000 }
    expect(tickWalker(s, 1000, bounds, 100, false, rWalk).x).toBe(100)
  })
  it('period end swaps mode deterministically via rand', () => {
    const s = { ...createWalker(100), mode: 'walk' as const, remainingMs: 100 }
    const next = tickWalker(s, 200, bounds, 100, false, () => 0.0) // low rand -> pause
    expect(next.mode).toBe('pause')
    expect(next.remainingMs).toBeGreaterThan(0)
  })
})
