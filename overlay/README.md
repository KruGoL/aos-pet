# aos-pet desktop overlay

The capsule's ASCII pet, walking along the bottom of your real screen in a
transparent always-on-top strip. Click it to feed / play / wash / heal / open
`aos chat`. State comes straight from the capsule — the overlay never invents
anything, it only renders KV truth.

```
        ┌ thought bubble ("еда?" when hungry, ailment name when ill, zzz…)
 (o.o) ─┘
 /|_|\   ← walks left/right, blinks, colour = alert level
─────────────────────────────────────────────── your taskbar ──
```

## How it connects

```
capsule ──MCP──> tools/pet.py serve  (HTTP 127.0.0.1:8737, in WSL on Windows)
                        ↑
                Electron overlay (this folder) polls /status, POSTs /action
```

No push ABI exists in AOS, so the overlay polls every 5 s; between polls the
walk animation is local. Feeding the pet from `aos chat` shows up on screen
within one poll — same state, two doors.

## Run

Windows (Node ≥ 20 required; AOS + aos-mcp broker installed in WSL):

```
overlay\pet-overlay.cmd        # or: npm install && npm start
```

The app probes the bridge on `127.0.0.1:8737` and starts it inside WSL
automatically when missing (the bridge in turn starts the AOS daemon). Tray
dot menu: show/hide, open chat, autostart with system, quit.

Linux / macOS: `npm install && npm start` — the bridge is spawned natively
(`python3 tools/pet.py serve`). Wayland caveat: transparency + always-on-top
depend on your compositor; X11 is fine.

Env knobs: `PET_BRIDGE_PORT` (default 8737), `PET_BRIDGE_CMD` (override the
bridge spawn command entirely).

## Develop

```
npm test          # vitest: emotion mapping + walker state machine
npx tsc --noEmit  # typecheck
npm start         # vite build + electron
```

Bridge unit tests live next to the bridge:
`cd ../tools && python3 -m unittest test_bridge -v` (run inside WSL).

Layout: `electron/` — shell (window flags, tray, click-through, process
spawns); `src/core/` — pure logic, unit-tested, no DOM; `src/render/` — DOM
widgets; `src/main.ts` — glue + PixiJS stage.

Packaging into a real installer (electron-builder) is deliberately not done
yet — v1 ships as `npm start` + the `.cmd` launcher.
