#!/usr/bin/env bash
# Guided on-screen test of the resident popup. Development tool.
#
#   tools/try-resident.sh [cycles]      (default: 5 automatic show/hide cycles)
#
# It starts PopUpAI hidden, as autostart would, against a fake Ollama server and a
# temporary configuration. After a countdown it shows and hides the popup by itself
# (over D-Bus, like `popupai --toggle` would), measures how long each appearance takes
# from the application's own timing logs, then shows it a last time for you to ask a
# real question. Everything it starts is stopped on exit.
#
# While it runs it holds the D-Bus name io.github.popupai.PopUpAI on your session bus.
# Timings cover the local application only; the model's own latency is separate.
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

PopUpAI is running hidden. In a moment the popup will appear and disappear by itself
$cycles times (about 4 seconds per cycle). Just watch the bottom-right corner:
does it show up every time, in the same place, ready for typing?
Then it stays up for you to ask a real question.

EOF
for n in 5 4 3 2 1; do
    printf 'Starting in %s...\r' "$n"
    sleep 1
done
echo

for i in $(seq 1 "$cycles"); do
    printf 'cycle %s/%s: show... ' "$i" "$cycles"
    call Show
    sleep 2.5
    printf 'hide\n'
    call Hide
    sleep 1.5
done

echo
echo "Last time: the popup stays up for 25 seconds. Type a question and press Enter,"
echo "try Ctrl+. while the answer streams, then click on another window (or press Esc)."
call Show
for _ in $(seq 1 250); do
    # The popup closes itself on focus loss or Esc: stop waiting once that happens.
    [ "$(grep -c 'popup dismissed' "$tmp/app.log")" -gt "$cycles" ] && break
    sleep 0.1
done
sleep 0.3
"$bin" --quit || true
sleep 0.5

echo
echo "================ timings (local application only) ================"
LOG="$tmp/app.log" python3 -I - <<'PY'
import os, re, statistics
from datetime import datetime

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

hides = sum(1 for _, e in events if e.startswith("popup dismissed"))
lost = [e for _, e in events if "reason=focus lost" in e]
print(f"dismissals: {hides} (focus lost: {len(lost)})")
for _, e in events:
    if e.startswith(("request started", "request finished", "request failed")):
        print("  ", e)
PY

echo
echo "--- requests the fake server saw (method and path only, never any content):"
cat "$tmp/fake.log"
