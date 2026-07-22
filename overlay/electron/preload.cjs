const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('petShell', {
  setInteractive: on => ipcRenderer.send('pet:interactive', !!on),
  openChat: () => ipcRenderer.send('pet:open-chat'),
  quit: () => ipcRenderer.send('pet:quit'),
  getAutostart: () => ipcRenderer.invoke('pet:get-autostart'),
  setAutostart: on => ipcRenderer.send('pet:set-autostart', !!on),
})
