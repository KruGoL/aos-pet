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
const GROUND_Y = 150

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

  const interactive = () => window.petShell.setInteractive(overPet || overMenu || ui.menuOpen())
  pet.on('pointerover', () => { overPet = true; interactive() })
  pet.on('pointerout', () => { overPet = false; setTimeout(interactive, 50) })
  const menuEnter = () => { overMenu = true; interactive() }
  const menuLeave = () => { overMenu = false; setTimeout(interactive, 50) }

  async function refresh() { presence = await fetchStatus() }
  setInterval(refresh, POLL_MS)
  refresh()

  async function act(tool: string, args?: object) {
    ui.hideMenu()
    const res = await postAction(tool, args).catch(() => ({ error: 'мост молчит' }))
    resultBubbleText = (res as any).message ?? (res as any).error ?? 'готово'
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
      `еда  [${ui.bar(s.fullness)}] ${s.fullness ?? '?'}   счастье [${ui.bar(s.happiness)}] ${s.happiness ?? '?'}\n` +
      `сон  [${ui.bar(s.energy)}] ${s.energy ?? '?'}   чистота [${ui.bar(s.cleanliness)}] ${s.cleanliness ?? '?'}\n` +
      (tail ? `\n${tail}` : ''), pet.x)
  }

  pet.on('pointertap', () => {
    if (ui.menuOpen()) { ui.hideMenu(); interactive(); return }
    if (presence.kind === 'no-pet') {
      ui.showAdopt(pet.x, name => act('adopt', { name }))
      return
    }
    ui.showMenu([
      { icon: '🍖', title: 'покормить', run: () => act('feed') },
      { icon: '⚽', title: 'поиграть', run: () => act('play') },
      { icon: '🧼', title: 'помыть', run: () => act('clean') },
      { icon: '💊', title: 'полечить', run: () => act('heal') },
      { icon: '😴', title: 'сон', run: () => act('sleep') },
      { icon: '📊', title: 'статус', run: openPanel },
      { icon: '💬', title: 'aos chat', run: () => { ui.hideMenu(); window.petShell.openChat() } },
      { icon: '✖', title: 'выход', run: () => window.petShell.quit() },
    ], pet.x + pet.width / 2, GROUND_Y - 40, menuEnter, menuLeave)
  })

  app.ticker.add(ticker => {
    const now = performance.now()
    const emotion = emotionFor(presence)
    const energy = presence.kind === 'ok' ? presence.status.energy ?? 50 : 0

    walker = tickWalker(walker, ticker.deltaMS, { min: 8, max: innerWidth - 160 },
      energy, emotion.asleep, Math.random)

    if (now - lastFrameSwap > FRAME_MS) { frame++; lastFrameSwap = now }
    pet.text = emotion.frames[frame % emotion.frames.length] ?? '(?)'
    ;(pet.style as TextStyle).fill = TINT_COLORS[emotion.tint] ?? TINT_COLORS.ok
    pet.x = walker.x
    pet.y = GROUND_Y - pet.height

    const bubble = now < resultBubbleUntil ? resultBubbleText : emotion.bubble
    ui.showBubble(bubble, walker.x, pet.y)
  })
}

boot()
