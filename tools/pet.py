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
  pet.py line                # print the cached one-liner (instant, no MCP)
  pet.py tmux                # print tmux setup for an always-visible pet
"""
import json
import os
import re
import subprocess
import sys
import threading
import time

AOS = os.path.expanduser("~/.aos/bin/aos")
HOME = os.path.expanduser("~")
LINE_CACHE = os.path.join(HOME, ".pet-line")

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
    """One long-lived MCP session. Pass timeout=None for daemon use."""

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
        if self.timeout is not None:
            self.timer = threading.Timer(self.timeout, self._kill)
            self.timer.start()
        self._handshake()

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
                kind = spec.get("type")
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
        rid = self.next_id
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": rid, "method": "tools/call",
                    "params": {"name": tool, "arguments": args or {}}})
        return self._pump(rid)

    def close(self):
        timer = getattr(self, "timer", None)
        if timer:
            timer.cancel()
        proc = getattr(self, "proc", None)
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
    if result is None:
        return None, f"error: {response.get('error')}"
    texts = [c.get("text", "") for c in result.get("content", [])
             if c.get("type") == "text"]
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


def show(tool, response):
    data, err = unpack(response)
    if err:
        print(f"[{tool}] {err}")
        return
    if isinstance(data, dict) and "log" in data and "opponent" in data:
        print(f"\n  {data.get('taunt', '')}")
        print()
        for line in data.get("log", []):
            print(f"    {line}")
        print()
        print(f"  >> {data.get('message', '')}")
        print(f"     hp left: you {data.get('my_hp_left')} / them {data.get('foe_hp_left')}"
              f"  ·  victories: {data.get('victories')}")
        return
    # A GameView wraps the pet plus the round's state.
    if isinstance(data, dict) and "pet" in data and isinstance(data["pet"], dict):
        print()
        print(data["pet"].get("display", ""))
        print(f"  >> {data.get('message', '')}")
        if data.get("active"):
            print(f"  ({data.get('guesses_left')} guesses left, range {data.get('range')})")
        return
    if isinstance(data, dict) and "display" in data:
        print()
        print(data["display"])
        print(f"  >> {data.get('message', '')}")
    elif isinstance(data, dict) and "seen_count" in data:
        print(f"\n  {data.get('name')} — moments witnessed: "
              f"{data.get('seen_count')} of {data.get('total')}")
        if data.get("now"):
            print(f"  right now: {data['now']}")
        for label in data.get("seen", []):
            print(f"    * {label}")
        if not data.get("seen"):
            print("    (none yet — leave it be for a while)")
    elif isinstance(data, dict) and "alerts" in data:
        print(f"\n  Recent events for {data.get('name')}:")
        if not data["alerts"]:
            print("    (nothing yet)")
        for a in data["alerts"]:
            print(f"    - {a['message']}")
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
        frames = data.get("frames") or [data.get("display", "")]
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
    print(f"  {data.get('message','')}")
    lo, hi = 1, 20
    try:
        lo, hi = (int(x) for x in str(data.get("range", "1-20")).split("-"))
    except ValueError:
        pass

    while True:
        value = (lo + hi) // 2
        data, err = unpack(session.call("pet_game_guess", {"value": value}))
        if err:
            print(err)
            return
        message = data.get("message", "")
        print(f"  guess {value:>2}  ->  {message}")
        if not data.get("active"):
            print()
            print(data.get("pet", {}).get("display", ""))
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
    print(f"pet daemon: refreshing {LINE_CACHE} every {interval}s "
          f"(Ctrl+C to stop)", file=sys.stderr)
    while True:
        try:
            if session is None:
                session = Session(timeout=None)
            data, err = unpack(session.call("pet_status", {}))
            if err:
                raise RuntimeError(err)
            body = (data.get("line") if isinstance(data, dict) else None) or "pet: ?"
            lvl = (data.get("level") if isinstance(data, dict) else None) or "ok"
            style = TMUX_STYLE.get(lvl, "")
            line = f"{style}{body}#[default]" if style else body
        except KeyboardInterrupt:
            raise
        except Exception as exc:
            line = "pet: offline"
            print(f"pet daemon: {exc}", file=sys.stderr)
            if session:
                session.close()
            session = None
        try:
            with open(LINE_CACHE, "w") as fh:
                fh.write(line + "\n")
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


def main():
    argv = sys.argv[1:] or ["status"]
    cmd = argv[0]

    # These never need an MCP session.
    if cmd == "line":
        print_line()
        return
    if cmd == "tmux":
        print(TMUX_HELP)
        return
    if cmd == "daemon":
        try:
            daemon(int(argv[1]) if len(argv) > 1 else 10)
        except KeyboardInterrupt:
            print()
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
    except KeyboardInterrupt:
        print()
    finally:
        session.close()


if __name__ == "__main__":
    main()
