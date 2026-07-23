// Electron shell for the aos-pet desktop overlay. All game logic lives in the
// renderer; this file owns the OS-facing seams: window flags, click-through,
// tray, autostart, and spawning the WSL bridge / aos chat terminal.
const { app, BrowserWindow, Tray, Menu, ipcMain, screen, nativeImage } = require('electron')
const { spawn } = require('child_process')
const http = require('http')
const path = require('path')

const STRIP_HEIGHT = 220
const BRIDGE_PORT = process.env.PET_BRIDGE_PORT || '8737'
let win = null
let tray = null

function repoRoot() { return path.resolve(__dirname, '..', '..') }

function toWslPath(winPath) {
  // C:\work\x -> /mnt/c/work/x
  return winPath.replace(/^([A-Za-z]):\\/, (_, d) => `/mnt/${d.toLowerCase()}/`).replace(/\\/g, '/')
}

function probeBridge(cb) {
  const req = http.get({ host: '127.0.0.1', port: BRIDGE_PORT, path: '/health', timeout: 1500 },
    res => { res.resume(); cb(res.statusCode === 200) })
  req.on('error', () => cb(false))
  req.on('timeout', () => { req.destroy(); cb(false) })
}

function spawnBridge() {
  if (process.env.PET_BRIDGE_CMD) {
    spawn(process.env.PET_BRIDGE_CMD, { shell: true, detached: true, stdio: 'ignore' }).unref()
    return
  }
  const script = path.join(repoRoot(), 'tools', 'pet.py')
  if (process.platform === 'win32') {
    // setsid + nohup: survive the wsl.exe wrapper exiting.
    const wsl = `cd ~ && setsid nohup python3 '${toWslPath(script)}' serve ${BRIDGE_PORT} >/tmp/pet-bridge.log 2>&1 & sleep 1`
    spawn('wsl.exe', ['-e', 'bash', '-lc', wsl], { detached: true, stdio: 'ignore' }).unref()
  } else {
    spawn('python3', [script, 'serve', BRIDGE_PORT], { detached: true, stdio: 'ignore' }).unref()
  }
}

// Chat opens inside tmux on purpose: the user's tmux status bar carries the
// always-visible mini pet (see tools/pet.py tmux), so the pet stays in the
// corner of the chat too. -A reuses the session instead of stacking new ones.
const CHAT_CMD = 'cd ~ && exec tmux new-session -A -s pet "~/.aos/bin/aos chat"'

function openChat() {
  if (process.platform === 'win32') {
    spawn('wt.exe', ['wsl.exe', '-e', 'bash', '-lc', CHAT_CMD],
      { detached: true, stdio: 'ignore', shell: false }).unref()
  } else if (process.platform === 'darwin') {
    spawn('osascript', ['-e', `tell app "Terminal" to do script "bash -lc '${CHAT_CMD.replace('~/.aos/bin/aos', 'aos')}'"`],
      { detached: true, stdio: 'ignore' }).unref()
  } else {
    spawn('x-terminal-emulator', ['-e', 'bash', '-lc', CHAT_CMD.replace('~/.aos/bin/aos', 'aos')],
      { detached: true, stdio: 'ignore' }).unref()
  }
}

function trayIcon() {
  // 16x16 solid rounded dot, drawn in-process so we ship no binary asset.
  const size = 16, buf = Buffer.alloc(size * size * 4)
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    const dx = x - 7.5, dy = y - 7.5, i = (y * size + x) * 4
    if (dx * dx + dy * dy <= 49) { buf[i] = 60; buf[i + 1] = 220; buf[i + 2] = 130; buf[i + 3] = 255 }
  }
  return nativeImage.createFromBuffer(buf, { width: size, height: size })
}

function createWindow() {
  const wa = screen.getPrimaryDisplay().workArea
  win = new BrowserWindow({
    x: wa.x, y: wa.y + wa.height - STRIP_HEIGHT, width: wa.width, height: STRIP_HEIGHT,
    frame: false, transparent: true, alwaysOnTop: true, skipTaskbar: true,
    resizable: false, movable: false, hasShadow: false, focusable: true,
    webPreferences: { preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true },
  })
  win.setAlwaysOnTop(true, 'screen-saver')
  win.setIgnoreMouseEvents(true, { forward: true })
  win.loadFile(path.join(__dirname, '..', 'dist', 'index.html'))
}

app.whenReady().then(() => {
  ipcMain.on('pet:interactive', (_e, on) => win && win.setIgnoreMouseEvents(!on, { forward: true }))
  ipcMain.on('pet:open-chat', openChat)
  ipcMain.on('pet:quit', () => app.quit())
  ipcMain.handle('pet:get-autostart', () => app.getLoginItemSettings().openAtLogin)
  ipcMain.on('pet:set-autostart', (_e, on) => app.setLoginItemSettings({ openAtLogin: !!on }))

  tray = new Tray(trayIcon())
  tray.setToolTip('aos-pet')
  tray.setContextMenu(Menu.buildFromTemplate([
    { label: 'Show/Hide pet', click: () => win && (win.isVisible() ? win.hide() : win.show()) },
    { label: 'Open aos chat', click: openChat },
    { type: 'separator' },
    { label: 'Start with system', type: 'checkbox',
      checked: app.getLoginItemSettings().openAtLogin,
      click: item => app.setLoginItemSettings({ openAtLogin: item.checked }) },
    { type: 'separator' },
    { label: 'Quit', click: () => app.quit() },
  ]))

  probeBridge(ok => { if (!ok) spawnBridge() })
  createWindow()
})

app.on('window-all-closed', () => app.quit())
