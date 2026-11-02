#!/usr/bin/env bash
# Guided on-screen test of the resident popup. Development tool.
#
#   tools/try-resident.sh [cycles]      (default: 5 automatic show/hide cycles)
#   COUNTDOWN=30 tools/try-resident.sh  (seconds before the first show; default 5)
#
# It starts PopUpAI hidden, as autostart would, against a fake Ollama server and a
# temporary configuration. After a countdown it shows and hides the popup by itself
# (over D-Bus, like `popupai --toggle` would), then shows it a last time for you to ask
# a real question. From the first show to the end it prints NOTHING: terminal output
# can make the window that hosts the terminal take the keyboard focus, which would
# close the popup (it closes when it loses focus) and corrupt the measurement.
#
# Afterwards it prints, from the application's own timing logs: the latency of each
# appearance, how long each one stayed up and what ended it, and where the full log
# was kept (target/try-resident.log). Timings cover the local application only; the
# compositor's own display latency and the model's latency are separate.
#
# While it runs it holds the D-Bus name io.github.popupai.PopUpAI on your session bus.
# Everything it starts is stopped on exit.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cycles="${1:-5}"
tmp="$(mktemp -d)"
fake=""
app=""
cleanup() {
    [ -n "$app" ] && kill "$app" 2>/dev/null || true
    [ -n "$fake" ] && kill "$fake" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

echo "Building..."
cargo build -p popupai-app >/dev/null 2>&1 || { echo "build failed: run 'cargo build -p popupai-app'"; exit 1; }
bin=target/debug/popupai-app

port="$(python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 -I tools/fake_ollama.py --port "$port" >"$tmp/fake.log" 2>&1 &
fake=$!
for _ in $(seq 1 50); do
    curl -sf "http://127.0.0.1:$port/api/tags" >/dev/null 2>&1 && break
    sleep 0.1
done
cat >"$tmp/config.toml" <<EOF
default_provider = "fake-ollama"

[[provider]]
id = "fake-ollama"
kind = "ollama"
name = "Fake Ollama"
endpoint = "http://127.0.0.1:$port"
model = "fake-model"
EOF
chmod 600 "$tmp/config.toml"

call() { # call Show|Hide
    dbus-send --session --dest=io.github.popupai.PopUpAI --type=method_call \
        /io/github/popupai/PopUpAI "io.github.popupai.PopUpAI.$1"
}

POPUPAI_CONFIG="$tmp/config.toml" QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1 \
    "$bin" --background >"$tmp/app.log" 2>&1 &
app=$!
for _ in $(seq 1 60); do
    grep -q "claimed the instance name" "$tmp/app.log" 2>/dev/null && break
    sleep 0.1
done
grep -q "claimed the instance name" "$tmp/app.log" || { echo "PopUpAI did not start:"; cat "$tmp/app.log"; exit 1; }

cat <<EOF

PopUpAI is running hidden. After the countdown the popup will appear and disappear by
itself $cycles times (about 4 seconds per cycle), then stay up for 25 seconds.

  - During the cycles: just watch the bottom-right corner. Does it show up every
    time, in the same place, ready for typing? Please do not click or type.
  - During the last 25 seconds: type a question and press Enter, try Ctrl+. while the
    answer streams, then click on another window (or press Esc).

Nothing will be printed until it is all over. The results appear at the end.

EOF
for n in $(seq "${COUNTDOWN:-5}" -1 1); do
    printf 'Starting in %s...  \r' "$n"
    sleep 1
done
echo "Running... (silent until the end)"

for _ in $(seq 1 "$cycles"); do
    call Show
    sleep 2.5
    call Hide
    sleep 1.5
done

call Show
for _ in $(seq 1 250); do
    # The popup closes itself on focus loss or Esc: stop waiting once that happens.
    [ "$(grep -c 'popup dismissed' "$tmp/app.log")" -gt "$cycles" ] && break
    sleep 0.1
done
sleep 0.3
"$bin" --quit || true
sleep 0.5

mkdir -p target
cp "$tmp/app.log" target/try-resident.log

echo
echo "================ timings (local application only) ================"
LOG="$tmp/app.log" CYCLES="$cycles" python3 -I - <<'PY'
import os, re, statistics
from datetime import datetime

cycles = int(os.environ["CYCLES"])
pat = re.compile(r"^(\S+Z)\s+\w+\s+(.*)$")
events = []
for line in open(os.environ["LOG"], errors="replace"):
    m = pat.match(line.strip())
    if m:
        ts = datetime.strptime(m.group(1).rstrip("Z"), "%Y-%m-%dT%H:%M:%S.%f")
        events.append((ts, m.group(2)))

def ms(a, b):
    return (b - a).total_seconds() * 1000.0

rows = []
for i, (t0, text) in enumerate(events):
    if text != "remote command received command=show":
        continue
    row = {"cmd": t0}
    for t, e in events[i + 1:]:
        if ms(t0, t) > 1500:
            break
        for key, needle in (("opened", "popup opened"), ("visible", "ui event=visible"),
                            ("active", "ui event=active"), ("frame", "ui event=first frame")):
            if key not in row and e.startswith(needle):
                row[key] = ms(t0, t)
    rows.append(row)

print(f"{'show':>5} {'opened':>8} {'visible':>8} {'active':>8} {'1st frame':>10}   (ms after the command)")
for n, r in enumerate(rows, 1):
    f = lambda k: f"{r[k]:8.1f}" if k in r else "       -"
    print(f"{n:>5} {f('opened')} {f('visible')} {f('active')} {f('frame'):>10}")
for key, label in (("active", "command to active (keyboard ready)"), ("frame", "command to first frame")):
    vals = [r[key] for r in rows if key in r]
    if vals:
        print(f"{label}: min {min(vals):.0f}  median {statistics.median(vals):.0f}  max {max(vals):.0f} ms over {len(vals)} shows")
missing = [n for n, r in enumerate(rows, 1) if "active" not in r]
if missing:
    print(f"WARNING: the popup did not become active after show(s) {missing}")

print()
print(f"{'show':>5} {'stayed up':>10}  ended by")
early = 0
for n, r in enumerate(rows, 1):
    t0 = r["cmd"]
    end = next(((t, e) for t, e in events if t > t0 and e.startswith("popup dismissed")), None)
    if end is None:
        print(f"{n:>5} {'-':>10}  still up when the application was quit")
        continue
    reason = end[1].split("reason=")[-1]
    lived = ms(t0, end[0])
    note = ""
    if n <= cycles and reason == "focus lost":
        early += 1
        note = "   <-- closed itself before the scripted Hide (at 2500 ms)"
    print(f"{n:>5} {lived:8.0f}ms  {reason}{note}")
print(f"automatic cycles that closed themselves by losing focus: {early} of {cycles}")

for _, e in events:
    if e.startswith(("request started", "request finished", "request failed")):
        print("  ", e)
PY

echo
echo "Full application log kept in: target/try-resident.log"
echo "--- requests the fake server saw (method and path only, never any content):"
cat "$tmp/fake.log"
