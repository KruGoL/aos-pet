import { Application, Text, TextStyle } from 'pixi.js'
import { fetchAlerts, fetchStatus, postAction } from './core/api'
import { emotionFor, type Presence } from './core/emotion'
import { createWalker, tickWalker } from './core/walker'
import * as ui from './render/widgets'

declare global {
  interface Window {
    petShell: {
      setInteractive(on: boolean): void
      openChat(): void
      quit(): void
      getAutostart(): Promise<boolean>
      setAutostart(on: boolean): void
    }
  }
}

const TINT_COLORS: Record<string, number> = {
  ok: 0xd8f3dc, resting: 0x90caf9, warn: 0xffd166, critical: 0xff6b6b, offline: 0x9e9e9e,
}
const POLL_MS = 5000
const FRAME_MS = 800
const GROUND_MARGIN = 20      // gap between the pet's feet and the window bottom
const GRAVITY = 2200          // px/s^2 for the fall after a drag
const MAX_FALL = 1400         // px/s terminal velocity

async function boot() {
  const app = new Application()
  await app.init({ backgroundAlpha: 0, resizeTo: window, antialias: false })
  document.getElementById('app')!.appendChild(app.canvas)

  const style = new TextStyle({
    fontFamily: 'Cascadia Mono, Consolas, monospace', fontSize: 18, lineHeight: 20,
    fill: 0xd8f3dc, dropShadow: { blur: 4, distance: 0, alpha: 0.8 },
  })
  const pet = new Text({ text: '(o.o)', style })
  pet.eventMode = 'static'
  pet.cursor = 'pointer'
  app.stage.addChild(pet)

  let presence: Presence = { kind: 'waking' }
  let walker = createWalker(innerWidth / 3)
  let frame = 0
  let lastFrameSwap = 0
  let resultBubbleUntil = 0
  let resultBubbleText = ''
  let overPet = false
  let overMenu = false
  let dragging = false
  let dragMoved = false
  let suppressTap = false
  let airY: number | null = null   // pet top while airborne; null = standing
  let fallSpeed = 0

  const groundTop = () => innerHeight - GROUND_MARGIN - pet.height

  const interactive = () => window.petShell.setInteractive(
    dragging || overPet || overMenu || ui.menuOpen() || ui.panelOpen())
  pet.on('pointerover', () => { overPet = true; interactive() })
  pet.on('pointerout', () => { overPet = false; setTimeout(interactive, 50) })
  const menuEnter = () => { overMenu = true; interactive() }
  const menuLeave = () => { overMenu = false; setTimeout(interactive, 50) }

  // Drag: grab the pet and carry it along the strip. A real drag suppresses
  // the tap that fires on release, so the menu does not pop open afterwards.
  pet.on('pointerdown', () => { dragging = true; dragMoved = false; interactive() })
  window.addEventListener('pointermove', ev => {
    if (!dragging) return
    dragMoved = true
    ui.hideMenu()
    const nx = Math.min(Math.max(ev.clientX - pet.width / 2, 8), innerWidth - 160)
    airY = Math.min(Math.max(ev.clientY - pet.height / 2, 0), groundTop())
    fallSpeed = 0
    walker = { ...walker, x: nx, mode: 'pause', remainingMs: 1500 }
  })
  window.addEventListener('pointerup', () => {
    if (!dragging) return
    dragging = false
    if (dragMoved) suppressTap = true
    setTimeout(interactive, 50)
  })

  async function refresh() { presence = await fetchStatus() }
  setInterval(refresh, POLL_MS)
  refresh()

  async function act(tool: string, args?: object) {
    ui.hideMenu()
    const res = await postAction(tool, args).catch(() => ({ error: 'bridge offline' }))
    resultBubbleText = (res as any).message ?? (res as any).error ?? 'done'
    resultBubbleUntil = performance.now() + 4000
    await refresh()
  }

  async function openPanel() {
    ui.hideMenu()
    if (presence.kind !== 'ok') return
    const s = presence.status
    const alerts = await fetchAlerts().catch(() => ({ alerts: [] as { message: string }[] }))
    const tail = (alerts.alerts ?? []).slice(-3).map(a => `- ${a.message}`).join('\n')
    ui.showPanel(
      `${s.name ?? 'pet'} · ${s.mood ?? ''}\n` +
      `food   [${ui.bar(s.fullness)}] ${s.fullness ?? '?'}   joy   [${ui.bar(s.happiness)}] ${s.happiness ?? '?'}\n` +
      `energy [${ui.bar(s.energy)}] ${s.energy ?? '?'}   clean [${ui.bar(s.cleanliness)}] ${s.cleanliness ?? '?'}\n` +
      (tail ? `\n${tail}` : ''), pet.x, pet.y, menuEnter, menuLeave)
    setTimeout(() => { if (ui.panelOpen()) { ui.hidePanel(); menuLeave() } }, 12_000)
  }

  pet.on('pointertap', () => {
    if (suppressTap) { suppressTap = false; return }
    if (ui.panelOpen()) { ui.hidePanel(); interactive(); return }
    if (ui.menuOpen()) { ui.hideMenu(); interactive(); return }
    if (presence.kind === 'no-pet') {
      ui.showAdopt(pet.x, pet.y, name => act('adopt', { name }), menuEnter, menuLeave)
      return
    }
    ui.showMenu([
      { icon: '🍖', title: 'feed', run: () => act('feed') },
      { icon: '⚽', title: 'play', run: () => act('play') },
      { icon: '🧼', title: 'wash', run: () => act('clean') },
      { icon: '💊', title: 'heal', run: () => act('heal') },
      { icon: '😴', title: 'sleep', run: () => act('sleep') },
      { icon: '📊', title: 'stats', run: openPanel },
      { icon: '💬', title: 'aos chat', run: () => { ui.hideMenu(); window.petShell.openChat() } },
      { icon: '✖', title: 'quit', run: () => window.petShell.quit() },
    ], pet.x + pet.width / 2, pet.y, menuEnter, menuLeave)
  })

  app.ticker.add(ticker => {
    const now = performance.now()
    const emotion = emotionFor(presence)
    const energy = presence.kind === 'ok' ? presence.status.energy ?? 50 : 0

    // Gravity: after a drag the pet falls back to the bottom edge.
    if (airY !== null && !dragging) {
      fallSpeed = Math.min(MAX_FALL, fallSpeed + GRAVITY * (ticker.deltaMS / 1000))
      airY += fallSpeed * (ticker.deltaMS / 1000)
      if (airY >= groundTop()) { airY = null; fallSpeed = 0 }
    }
    const airborne = airY !== null

    walker = tickWalker(walker, ticker.deltaMS, { min: 8, max: innerWidth - 160 },
      energy, emotion.asleep || airborne, Math.random)

    if (now - lastFrameSwap > FRAME_MS) { frame++; lastFrameSwap = now }
    pet.text = emotion.frames[frame % emotion.frames.length] ?? '(?)'
    ;(pet.style as TextStyle).fill = TINT_COLORS[emotion.tint] ?? TINT_COLORS.ok
    pet.x = walker.x
    pet.y = airY ?? groundTop()

    const bubble = now < resultBubbleUntil ? resultBubbleText : emotion.bubble
    ui.showBubble(bubble, walker.x, pet.y)
  })
}

boot()
