// Deterministic walking brain: a pure (state, dt, inputs, rand) -> state
// function so the whole behaviour is unit-testable without a renderer.

export interface WalkerState {
  x: number
  dir: 1 | -1
  mode: 'walk' | 'pause'
  remainingMs: number
}

export const BASE_SPEED_PX_S = 40

export function createWalker(startX: number): WalkerState {
  return { x: startX, dir: 1, mode: 'walk', remainingMs: 4000 }
}

export function tickWalker(
  s: WalkerState,
  dtMs: number,
  bounds: { min: number; max: number },
  energy: number,
  asleep: boolean,
  rand: () => number,
): WalkerState {
  if (asleep) return { ...s, mode: 'pause' }

  let { x, dir, mode, remainingMs } = s
  remainingMs -= dtMs
  if (remainingMs <= 0) {
    // Swap activity: rand < 0.4 -> pause 1-3 s, else walk 3-8 s.
    if (rand() < 0.4) { mode = 'pause'; remainingMs = 1000 + rand() * 2000 }
    else { mode = 'walk'; remainingMs = 3000 + rand() * 5000 }
    if (mode === 'walk' && rand() < 0.5) dir = dir === 1 ? -1 : 1
  }

  if (mode === 'walk') {
    const speed = BASE_SPEED_PX_S * (0.4 + 0.6 * Math.min(100, Math.max(0, energy)) / 100)
    x += dir * speed * (dtMs / 1000)
    if (x <= bounds.min) { x = bounds.min; dir = 1 }
    if (x >= bounds.max) { x = bounds.max; dir = -1 }
  }
  return { x, dir, mode, remainingMs }
}
