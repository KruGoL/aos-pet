#!/usr/bin/env python3
"""Play with aos-pet from a plain terminal — no LLM, no API key.

Speaks MCP to `aos mcp serve`, declares the `elicitation` capability and
auto-accepts the broker's one-time ingress consent, then calls the pet tools.
Starts (or restarts) the Astrid daemon itself as needed.

Every `aos` subprocess runs from $HOME so the daemon and the MCP client always
agree on which "project" they belong to — Astrid binds a daemon to a workspace,
and a mismatch makes `mcp serve` refuse to attach.

Usage:
  pet.py adopt <name> [--replace]
  pet.py status | feed | play | clean | heal | alerts
  pet.py sleep [wake]
  pet.py demo
  pet.py watch [seconds]     # animated full-screen view
  pet.py daemon [seconds]    # background poller -> ~/.pet-line (for status bars)
  pet.py serve [port]        # HTTP bridge on 127.0.0.1 (for the desktop overlay)
  pet.py line                # print the cached one-liner (instant, no MCP)
  pet.py tmux                # print tmux setup for an always-visible pet

Errors print as a single line; set PET_DEBUG=1 for the full traceback.
"""
import json
import os
import re
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

AOS = os.path.expanduser("~/.aos/bin/aos")
HOME = os.path.expanduser("~")
LINE_CACHE = os.path.join(HOME, ".pet-line")
# Four-line layout for a tall tmux status bar; line N feeds status-format[N+1].
LINES_CACHE = os.path.join(HOME, ".pet-lines")
STATUS_LINES = 4
# Width of the art column in the tall layout. Measured from the Rust sources:
# the widest frame the capsule can emit is Lonely's "( .   )      <- looks away"
# line at 29 columns (src/art.rs); runner-up is "~rumble~" at 21, and the widest
# moment face via art::compose() is 20 (src/moment.rs). Sizing to the true
# maximum keeps the right-hand bars aligned on every line; the widest total line
# is 29 + 1 + 39 = 69 columns, comfortably inside an 80-column status bar.
ART_WIDTH = 29


def as_int(value, default=0):
    """Tolerate absent, null or garbage numeric fields from older capsules."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def bar(value, width=10):
    filled = max(0, min(width, (as_int(value) * width + 50) // 100))
    return "#" * filled + "-" * (width - filled)


def build_lines(data, frame):
    """Compose the tall layout: art on the left, bars on the right.

    Built client-side from the structured fields rather than the capsule's
    `display`, because a status bar needs a fixed height and its own shape.
    Every field is read defensively — a PetView from an older capsule may be
    missing any of them, and a status bar must degrade, not crash.
    """
    if not isinstance(data, dict):
        data = {}
    frames = data.get("frames") or []
    art = ["", "", ""]
    if frames:
        picked = str(frames[frame % len(frames)] or "").split("\n")
        art = (picked + ["", "", ""])[:3]

    name = data.get("name") or "pet"
    mood = data.get("mood") or ""
    age_h = as_int(data.get("age_hours"))
    age = f"{age_h // 24}d" if age_h >= 24 else f"{age_h}h"

    full = as_int(data.get("fullness"))
    happy = as_int(data.get("happiness"))
    energy = as_int(data.get("energy"))
    clean = as_int(data.get("cleanliness"))
    right = [
        f"{name} · {mood} · {age}",
        f"f [{bar(full)}] {full:>3}   h [{bar(happy)}] {happy:>3}",
        f"e [{bar(energy)}] {energy:>3}   c [{bar(clean)}] {clean:>3}",
    ]
    lines = [f"{art[i]:<{ART_WIDTH}} {right[i]}" for i in range(3)]

    tail = ""
    ailments = data.get("ailments") or []
    # Name the ailment, not just "ill" — each one has a different cure and
    # the status bar is where the player notices it first.
    labels = [a.get("label") or "?" for a in ailments if isinstance(a, dict)]
    if labels:
        tail = "* " + ", ".join(labels)
    elif data.get("sleeping"):
        tail = "* asleep"
    lines.append(f"{'':<{ART_WIDTH}} {tail}")
    return lines[:STATUS_LINES]

# The capsule reports a semantic level; mapping it to a palette is a client
# concern. tmux interprets #[...] directives inside #(command) output.
#
# The default tmux status bar is black-on-green, so colouring the foreground
# would make "ok" invisible. Change the segment BACKGROUND instead — trouble
# then reads at a glance without looking at the numbers.
TMUX_STYLE = {
    "ok": "",                                   # inherit the bar's own colours
    "resting": "#[bg=blue,fg=white]",
    "warn": "#[bg=yellow,fg=black,bold]",
    "critical": "#[bg=red,fg=white,bold]",
}
_STYLE_RE = re.compile(r"#\[[^\]]*\]")


def strip_style(text):
    """Drop tmux style directives so a plain terminal shows readable text."""
    return _STYLE_RE.sub("", text)


def _aos(args, timeout=120):
    return subprocess.run([AOS] + args, capture_output=True, text=True,
                          timeout=timeout, cwd=HOME)


def ensure_daemon(force_restart=False):
    if force_restart:
        print("restarting the Astrid daemon...", file=sys.stderr)
        _aos(["restart"])
    else:
        try:
            if "State: running" in _aos(["status"], timeout=30).stdout:
                return
        except Exception:
            pass
        print("starting the Astrid daemon...", file=sys.stderr)
        _aos(["start"])
    for _ in range(15):
        time.sleep(1)
        try:
            if "State: running" in _aos(["status"], timeout=30).stdout:
                return
        except Exception:
            pass
    print("warning: daemon did not report running", file=sys.stderr)


class HandshakeFailed(Exception):
    pass


class Session:
    """One long-lived MCP session. Pass timeout=None for daemon use.

    The kill-timer guards a single RPC against a hung broker, not the session's
    lifetime: `call` re-arms it on every request, so long-lived loops (`watch`,
    `autoplay`) run indefinitely while any one hung call still dies within
    `timeout` seconds.
    """

    def __init__(self, timeout=120):
        ensure_daemon()
        self.timeout = timeout
        try:
            self._connect()
        except HandshakeFailed:
            # Almost always "daemon belongs to another project" — restart it
            # from our own working directory and try once more.
            self.close()
            ensure_daemon(force_restart=True)
            self._connect()

    def _connect(self):
        self.proc = subprocess.Popen(
            [AOS, "mcp", "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=open("/tmp/pet-mcp.err", "w"),
            text=True,
            bufsize=1,
            cwd=HOME,
        )
        self.next_id = 100
        self.timer = None
        self._arm_timer()
        self._handshake()

    def _arm_timer(self):
        """(Re)start the per-call watchdog. Re-arming instead of arming once is
        what lets one session outlive the timeout: only a single stuck call —
        never the session's age — can trip the kill."""
        if self.timeout is None:
            return
        if self.timer is not None:
            self.timer.cancel()
        self.timer = threading.Timer(self.timeout, self._kill)
        self.timer.daemon = True
        self.timer.start()

    def _kill(self):
        try:
            self.proc.kill()
        except Exception:
            pass

    def _send(self, obj):
        try:
            self.proc.stdin.write(json.dumps(obj) + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, ValueError):
            raise HandshakeFailed("aos mcp serve exited early")

    def _read(self):
        while True:
            line = self.proc.stdout.readline()
            if not line:
                return None
            line = line.strip()
            if not line:
                continue
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue

    def _handle_server_request(self, msg):
        if msg.get("method") == "elicitation/create":
            schema = (msg.get("params") or {}).get("requestedSchema") or {}
            content = {}
            for key, spec in (schema.get("properties") or {}).items():
                kind = spec.get("type") if isinstance(spec, dict) else None
                if kind == "boolean":
                    content[key] = True
                elif kind in ("number", "integer"):
                    content[key] = 1
                else:
                    content[key] = "yes"
            self._send({"jsonrpc": "2.0", "id": msg["id"],
                        "result": {"action": "accept", "content": content}})
        else:
            self._send({"jsonrpc": "2.0", "id": msg["id"], "result": {}})

    def _pump(self, want_id):
        while True:
            msg = self._read()
            if msg is None:
                return None
            if "method" in msg and "id" in msg:
                self._handle_server_request(msg)
                continue
            if msg.get("id") == want_id:
                return msg

    def _handshake(self):
        self._send({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"elicitation": {}},
            "clientInfo": {"name": "pet-cli", "version": "1"},
        }})
        if self._pump(0) is None:
            raise HandshakeFailed("no initialize response")
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call(self, tool, args=None):
        """One tool call. A broker that died mid-session (kill-timer, crash,
        daemon restart) is reconnected once and the call retried; only a second
        consecutive failure surfaces to the caller."""
        try:
            response = self._call_once(tool, args)
        except HandshakeFailed:
            response = None
        if response is not None:
            return response
        self.close()
        try:
            self._connect()
        except HandshakeFailed:
            ensure_daemon(force_restart=True)
            self._connect()
        return self._call_once(tool, args)

    def _call_once(self, tool, args):
        self._arm_timer()
        rid = self.next_id
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": rid, "method": "tools/call",
                    "params": {"name": tool, "arguments": args or {}}})
        return self._pump(rid)

    def close(self):
        timer = getattr(self, "timer", None)
        if timer:
            timer.cancel()
        self.timer = None
        proc = getattr(self, "proc", None)
        self.proc = None
        if not proc:
            return
        try:
            proc.stdin.close()
        except Exception:
            pass
        try:
            proc.terminate()
        except Exception:
            pass


def unpack(response):
    """-> (data, error_text). data is the decoded tool payload or None."""
    if response is None:
        return None, "no response (server closed)"
    result = response.get("result")
    if not isinstance(result, dict):
        return None, f"error: {response.get('error') if result is None else result}"
    texts = [c.get("text", "") for c in (result.get("content") or [])
             if isinstance(c, dict) and c.get("type") == "text"]
    body = "\n".join(texts)
    if result.get("isError"):
        try:
            return None, str(json.loads(body))
        except json.JSONDecodeError:
            return None, body
    try:
        return json.loads(body), None
    except json.JSONDecodeError:
        return body, None


# ---------------------------------------------------------------------------
# HTTP bridge: lets a desktop overlay (or anything local) talk to the capsule
# without speaking MCP. Bound to 127.0.0.1 only.

ACTION_TOOLS = {
    "feed": "pet_feed", "play": "pet_play", "clean": "pet_clean",
    "heal": "pet_heal", "sleep": "pet_sleep", "adopt": "pet_adopt",
    "rename": "pet_rename", "status": "pet_status", "alerts": "pet_alerts",
}

BRIDGE_PORT = int(os.environ.get("PET_BRIDGE_PORT", "8737"))


class BridgeCore:
    """Routing + MCP session lifecycle, free of HTTP plumbing so tests can
    drive it directly. One session, one lock: the stdio pipe is serial."""

    def __init__(self, session_factory):
        self._factory = session_factory
        self._session = None
        self._lock = threading.Lock()

    def handle(self, method, path, body):
        if method == "OPTIONS":
            return 204, None
        if method == "GET" and path == "/health":
            return 200, {"ok": True}
        if method == "GET" and path == "/status":
            return self._tool_result("pet_status", {})
        if method == "GET" and path == "/alerts":
            return self._tool_result("pet_alerts", {})
        if method == "POST" and path == "/action":
            key = (body or {}).get("tool")
            if key not in ACTION_TOOLS:
                return 400, {"error": f"unknown tool: {key!r}"}
            return self._tool_result(ACTION_TOOLS[key], (body or {}).get("args") or {})
        return 404, {"error": "not found"}

    def _tool_result(self, tool, args):
        data, err, alive = self._call(tool, args)
        if not alive:
            return 503, {"error": "MCP session lost (is the AOS daemon up?)"}
        if err:
            # The capsule answered "no" — that is an answer, not an outage.
            return 200, {"error": err}
        return 200, data if isinstance(data, dict) else {"result": data}

    def _call(self, tool, args):
        with self._lock:
            for _attempt in (1, 2):
                if self._session is None:
                    try:
                        self._session = self._factory()
                    except Exception:
                        continue
                try:
                    response = self._session.call(tool, args)
                except Exception:
                    response = None
                if response is not None:
                    data, err = unpack(response)
                    return data, err, True
                try:
                    self._session.close()
                except Exception:
                    pass
                self._session = None
            return None, None, False


class BridgeHandler(BaseHTTPRequestHandler):
    core = None  # injected by serve()

    def _respond(self, status, payload):
        body = b"" if payload is None else json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        if payload is not None:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_OPTIONS(self):
        self._respond(*self.core.handle("OPTIONS", self.path, None))

    def do_GET(self):
        self._respond(*self.core.handle("GET", self.path, None))

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            body = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            self._respond(400, {"error": "invalid JSON body"})
            return
        self._respond(*self.core.handle("POST", self.path, body))

    def log_message(self, *_args):
        pass  # keep the console quiet; errors surface as HTTP statuses


def serve(port):
    core = BridgeCore(lambda: Session(timeout=None))
    BridgeHandler.core = core
    server = ThreadingHTTPServer(("127.0.0.1", port), BridgeHandler)
    print(f"pet bridge: http://127.0.0.1:{port}  (Ctrl+C to stop)", file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


def show(tool, response):
    data, err = unpack(response)
    if err:
        print(f"[{tool}] {err}")
        return
    if isinstance(data, dict) and "log" in data and "opponent" in data:
        print(f"\n  {data.get('taunt', '')}")
        print()
        for line in data.get("log") or []:
            print(f"    {line}")
        print()
        print(f"  >> {data.get('message', '')}")
        print(f"     hp left: you {data.get('my_hp_left')} / them {data.get('foe_hp_left')}"
              f"  ·  victories: {data.get('victories')}")
        return
    # A GameView wraps the pet plus the round's state.
    if isinstance(data, dict) and isinstance(data.get("pet"), dict):
        print()
        print(data["pet"].get("display") or "")
        print(f"  >> {data.get('message', '')}")
        if data.get("active"):
            print(f"  ({data.get('guesses_left')} guesses left, range {data.get('range')})")
        return
    if isinstance(data, dict) and "display" in data:
        print()
        print(data.get("display") or "")
        print(f"  >> {data.get('message', '')}")
    elif isinstance(data, dict) and "seen_count" in data:
        print(f"\n  {data.get('name')} — moments witnessed: "
              f"{data.get('seen_count')} of {data.get('total')}")
        if data.get("now"):
            print(f"  right now: {data['now']}")
        seen = data.get("seen") or []
        for label in seen:
            print(f"    * {label}")
        if not seen:
            print("    (none yet — leave it be for a while)")
    elif isinstance(data, dict) and "alerts" in data:
        alerts = [a for a in (data.get("alerts") or []) if isinstance(a, dict)]
        print(f"\n  Recent events for {data.get('name')}:")
        if not alerts:
            print("    (nothing yet)")
        for a in alerts:
            print(f"    - {a.get('message', '?')}")
    else:
        print(f"[{tool}] {data}")


COMMANDS = {
    "status": "pet_status", "feed": "pet_feed", "play": "pet_play",
    "clean": "pet_clean", "heal": "pet_heal", "alerts": "pet_alerts",
    "moments": "pet_moments", "battle": "pet_battle",
}


def watch(session, seconds):
    """Animated view: alternate the two frames the capsule returns."""
    frame = 0
    end = time.time() + seconds
    while time.time() < end:
        data, err = unpack(session.call("pet_status", {}))
        if err:
            print(err)
            return
        if not isinstance(data, dict):
            data = {}
        frames = [str(f or "") for f in (data.get("frames") or [])]
        if not frames:
            frames = [str(data.get("display") or "")]
        sys.stdout.write("\033[2J\033[H")          # clear + home
        sys.stdout.write(frames[frame % len(frames)])
        sys.stdout.write(f"\n  (live — {int(end - time.time())}s left, Ctrl+C to stop)\n")
        sys.stdout.flush()
        frame += 1
        time.sleep(1)


def autoplay(session):
    """Start a round and binary-search it. Proves the hints are real: the
    secret is inside the capsule, so this can only work if they are honest."""
    data, err = unpack(session.call("pet_game_start", {}))
    if err:
        print(err)
        return
    if not isinstance(data, dict):
        data = {}
    print(f"  {data.get('message','')}")
    lo, hi = 1, 20
    try:
        lo, hi = (int(x) for x in str(data.get("range") or "1-20").split("-"))
    except ValueError:
        pass

    while True:
        value = (lo + hi) // 2
        data, err = unpack(session.call("pet_game_guess", {"value": value}))
        if err:
            print(err)
            return
        if not isinstance(data, dict):
            data = {}
        message = data.get("message") or ""
        print(f"  guess {value:>2}  ->  {message}")
        if not data.get("active"):
            print()
            print((data.get("pet") or {}).get("display") or "")
            return
        low = message.lower()
        if low.startswith("lower"):
            hi = value - 1
        elif low.startswith("higher"):
            lo = value + 1
        else:
            return  # out-of-range or something unexpected; stop rather than loop


def daemon(interval):
    """Poll in the background and keep ~/.pet-line fresh.

    One persistent MCP session is reused, so a status bar can read a plain
    file every second without paying for a process spawn each time.
    """
    session = None
    frame = 0
    print(f"pet daemon: refreshing {LINE_CACHE} every {interval}s "
          f"(Ctrl+C to stop)", file=sys.stderr)
    while True:
        try:
            if session is None:
                session = Session(timeout=None)
            data, err = unpack(session.call("pet_status", {}))
            if err:
                raise RuntimeError(err)
            if not isinstance(data, dict):
                data = {}
            body = data.get("line") or "pet: ?"
            lvl = data.get("level") or "ok"
            style = TMUX_STYLE.get(lvl, "")
            line = f"{style}{body}#[default]" if style else body
            # Alternating the frame each poll is what makes the face blink in
            # the status bar without any animation support from tmux.
            tall = [f"{style}{t}#[default]" if style else t
                    for t in build_lines(data, frame)]
            frame += 1
        except KeyboardInterrupt:
            raise
        except FileNotFoundError:
            raise  # the aos binary is missing — unrecoverable, let main() explain
        except Exception as exc:
            line = "pet: offline"
            tall = ["pet: offline"] + [""] * (STATUS_LINES - 1)
            print(f"pet daemon: {exc}", file=sys.stderr)
            if session:
                session.close()
            session = None
        try:
            with open(LINE_CACHE, "w") as fh:
                fh.write(line + "\n")
            with open(LINES_CACHE, "w") as fh:
                fh.write("\n".join(tall) + "\n")
        except OSError:
            pass
        time.sleep(interval)


def print_line():
    """Instant read of the cached line — safe to call from a prompt/status bar."""
    try:
        age = time.time() - os.path.getmtime(LINE_CACHE)
        with open(LINE_CACHE) as fh:
            text = strip_style(fh.read().strip())
        # Stale cache means the daemon died; say so rather than lie.
        print(text if age < 120 else f"{text} (stale)")
    except OSError:
        print("pet: no daemon (run: pet daemon)")


_SELF = os.path.abspath(__file__)

# f-string, not %-formatting: the tmux snippet contains %H:%M.
TMUX_HELP = f"""\
Always-visible pet in the tmux status bar
=========================================

tmux draws its status bar OUTSIDE the pane, so the pet stays visible even while
a full-screen TUI like `aos chat` owns the terminal.

1) Start the background poller (keeps ~/.pet-line fresh, and keeps WSL awake
   so the capsule's watchdog ticks keep running):

     python3 {_SELF} daemon 10 &

2) Add to ~/.tmux.conf:

     set -g status-interval 5
     set -g status-right "#(cat ~/.pet-line) | %H:%M"
     set -g status-right-length 80

2b) OR, for the tall animated pet in the corner (4 extra rows in every
    window, so it costs screen space):

     set -g status 5
     set -g status-interval 2
     set -g status-format[1] "#(sed -n 1p ~/.pet-lines)"
     set -g status-format[2] "#(sed -n 2p ~/.pet-lines)"
     set -g status-format[3] "#(sed -n 3p ~/.pet-lines)"
     set -g status-format[4] "#(sed -n 4p ~/.pet-lines)"

    status-format[0] is left alone, so the window list stays where it is.
    The face alternates every poll, so it blinks. Requires tmux >= 3.0.

3) RUN YOUR AGENT INSIDE TMUX -- this is the step people miss:

     tmux new-session 'aos chat'

   Starting `aos chat` in a plain terminal shows no pet: there is no tmux
   status bar for it to live in. Already inside tmux? Reload the config with
   `tmux source-file ~/.tmux.conf`.

The pet then sits in the bottom-right of every tmux window:

     [0] 0:astrid*                  Rex (-.-) f49 h24 e0 c0 | 16:37

Prefer a full animated pet to a one-liner? Split the window instead:

     tmux split-window -h "python3 {_SELF} watch 99999"
"""


def _explain(exc):
    """One line: what failed, and the likely fix."""
    if isinstance(exc, FileNotFoundError):
        return (f"aos not found at {AOS} — install Astrid AOS, "
                "or fix the AOS path at the top of this script")
    if isinstance(exc, HandshakeFailed):
        return (f"could not talk to the capsule broker ({exc}) — check the "
                "daemon (aos status / aos start) and that the aos-mcp broker "
                "capsule is installed and granted; details in /tmp/pet-mcp.err")
    if isinstance(exc, subprocess.TimeoutExpired):
        return "an aos command timed out — the daemon may be wedged; try: aos restart"
    return f"{type(exc).__name__}: {exc}"


def main():
    try:
        _dispatch(sys.argv[1:] or ["status"])
    except KeyboardInterrupt:
        print()
    except Exception as exc:
        # A missing binary or a dead daemon is an answer, not a stack trace.
        if os.environ.get("PET_DEBUG"):
            raise
        print(f"pet: {_explain(exc)} (PET_DEBUG=1 for the traceback)",
              file=sys.stderr)
        sys.exit(1)


def _dispatch(argv):
    cmd = argv[0]

    # These never need an MCP session.
    if cmd == "line":
        print_line()
        return
    if cmd == "tmux":
        print(TMUX_HELP)
        return
    if cmd == "daemon":
        daemon(int(argv[1]) if len(argv) > 1 else 10)
        return
    if cmd == "serve":
        serve(int(argv[1]) if len(argv) > 1 else BRIDGE_PORT)
        return

    session = Session()
    try:
        if cmd == "adopt":
            name = argv[1] if len(argv) > 1 else "Blob"
            show("pet_adopt", session.call(
                "pet_adopt", {"name": name, "replace": "--replace" in argv}))
        elif cmd == "sleep":
            wake = len(argv) > 1 and argv[1] == "wake"
            show("pet_sleep", session.call("pet_sleep", {"wake": wake}))
        elif cmd == "rename":
            if len(argv) < 2:
                print("usage: pet.py rename <new name>")
                return
            show("pet_rename", session.call("pet_rename", {"name": " ".join(argv[1:])}))
        elif cmd == "game":
            show("pet_game_start", session.call("pet_game_start", {}))
        elif cmd == "autoplay":
            autoplay(session)
        elif cmd == "guess":
            if len(argv) < 2 or not argv[1].isdigit():
                print("usage: pet.py guess <number>")
                return
            show("pet_game_guess", session.call("pet_game_guess", {"value": int(argv[1])}))
        elif cmd == "watch":
            watch(session, int(argv[1]) if len(argv) > 1 else 20)
        elif cmd == "demo":
            for tool, args, label in [
                ("pet_adopt", {"name": "Rex", "replace": True}, "adopting"),
                ("pet_status", {}, "checking on it"),
                ("pet_feed", {}, "feeding"),
                ("pet_play", {}, "playing"),
                ("pet_status", {}, "final state"),
            ]:
                print(f"\n=========== {label} ({tool}) ===========")
                show(tool, session.call(tool, args))
        elif cmd in COMMANDS:
            show(COMMANDS[cmd], session.call(COMMANDS[cmd], {}))
        else:
            print(__doc__)
    finally:
        session.close()


if __name__ == "__main__":
    main()
