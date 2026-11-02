#!/usr/bin/env bash
# Try the PopUpAI popup against the fake Ollama server, with a countdown so you can
# get ready. Everything it starts is stopped when it exits. Development tool.
#
#   tools/try-popup.sh
#
# It uses a temporary configuration and a free port, so it touches neither your real
# configuration nor anything already listening. The popup is one-shot: it closes
# when you press Esc, click elsewhere, or after 120 seconds.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

tmp="$(mktemp -d)"
fake=""
cleanup() {
    [ -n "$fake" ] && kill "$fake" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

echo "Building..."
cargo build -p popupai-app >/dev/null 2>&1 || { echo "build failed: run 'cargo build -p popupai-app'"; exit 1; }

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

cat <<'EOF'

What to try once the popup appears (bottom-right corner):
  1. Type a question and press Enter. The answer should arrive word by word.
  2. Press Ctrl+. (or the Stop button) while it streams: it should stop.
  3. Try Copy, then Esc, or click on another window: the popup should close.

EOF
for n in 5 4 3 2 1; do
    printf 'The popup appears in %s...\r' "$n"
    sleep 1
done
echo

POPUPAI_CONFIG="$tmp/config.toml" QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1 \
    timeout 120 target/debug/popupai-app 2>&1 | grep -vE 'sfinae|range_access' || true

echo
echo "--- requests the fake server saw (method and path only, never any content):"
cat "$tmp/fake.log"
