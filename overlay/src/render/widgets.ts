// DOM widgets floating above the Pixi pet: thought bubble, item menu, status
// panel, adopt form. DOM (not Pixi) because buttons/inputs are free here.

const widgets = () => document.getElementById('widgets')!

export function showBubble(text: string | null, x: number, y: number): void {
  let el = document.getElementById('pet-bubble') as HTMLDivElement | null
  if (!text) { el?.remove(); return }
  if (!el) {
    el = document.createElement('div')
    el.id = 'pet-bubble'
    el.className = 'bubble'
    widgets().appendChild(el)
  }
  el.textContent = text
  el.style.left = `${x}px`
  el.style.top = `${Math.max(0, y - 34)}px`
}

export interface MenuAction { icon: string; title: string; run: () => void }

export function showMenu(actions: MenuAction[], x: number, y: number,
                         onEnter: () => void, onLeave: () => void): void {
  hideMenu()
  const el = document.createElement('div')
  el.id = 'pet-menu'
  el.className = 'menu'
  for (const a of actions) {
    const b = document.createElement('button')
    b.textContent = a.icon
    b.title = a.title
    b.onclick = () => { a.run() }
    el.appendChild(b)
  }
  el.onmouseenter = onEnter
  el.onmouseleave = onLeave
  widgets().appendChild(el)
  const w = el.offsetWidth
  el.style.left = `${Math.max(4, Math.min(x - w / 2, innerWidth - w - 4))}px`
  el.style.top = `${Math.max(0, y - 56)}px`
}

export function hideMenu(): void { document.getElementById('pet-menu')?.remove() }
export function menuOpen(): boolean { return !!document.getElementById('pet-menu') }

export function showPanel(text: string, x: number): void {
  hidePanel()
  const el = document.createElement('div')
  el.id = 'pet-panel'
  el.className = 'panel'
  el.textContent = text
  el.onclick = hidePanel
  widgets().appendChild(el)
  el.style.left = `${Math.max(4, Math.min(x, innerWidth - el.offsetWidth - 4))}px`
  el.style.top = '4px'
}
export function hidePanel(): void { document.getElementById('pet-panel')?.remove() }

export function showAdopt(x: number, onAdopt: (name: string) => void): void {
  hideMenu()
  const el = document.createElement('div')
  el.id = 'pet-menu'
  el.className = 'menu adopt'
  const input = document.createElement('input')
  input.placeholder = 'имя питомца'
  const b = document.createElement('button')
  b.textContent = '🐣'
  b.title = 'завести'
  b.onclick = () => onAdopt(input.value.trim() || 'Blob')
  el.append(input, b)
  widgets().appendChild(el)
  el.style.left = `${Math.max(4, x - 60)}px`
  el.style.top = '120px'
  input.focus()
}

export function bar(value: number | undefined, width = 10): string {
  const v = Math.max(0, Math.min(100, value ?? 0))
  const filled = Math.round((v * width) / 100)
  return '#'.repeat(filled) + '-'.repeat(width - filled)
}
