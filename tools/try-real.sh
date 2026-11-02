#!/usr/bin/env bash
# Guided test of the application as a user would run it, on the REAL session: the global
# shortcut through the desktop portal, the tray icon, the popup, and a real answer from the
# local Ollama server. Development tool.
#
#   tools/try-real.sh               (90 seconds for you to use it; click the tray icon to open)
#   WINDOW=180 tools/try-real.sh
#
# It starts PopUpAI hidden, as autostart would, with a temporary configuration that uses
# only the local Ollama server (so the secret store is not touched) and a private
# XDG_CONFIG_HOME (so no autostart entry is read or written). Then it waits, printing
# nothing, after showing the popup once by itself. You then use it: set the shortcut if the
# popup says it has no key, press the shortcut, type a question, press Enter, and click elsewhere
# to close it. Afterwards it prints the application's own metadata-only log: what the portal
# said about the shortcut, whether the tray icon was shown, and each opening, request and
# dismissal. Prompts and answers are never in that log.
#
# Side effect that stays: the first time, the desktop records a shortcut entry for the
# application id (KDE: System Settings > Shortcuts). Remove it there if you do not want it.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

window="${WINDOW:-90}"
tmp="$(mktemp -d)"
app=""
cleanup() {
    [ -n "$app" ] && kill "$app" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

echo "Building..."
cargo build -p popupai-app >/dev/null 2>&1 || { echo "build failed: run 'cargo build -p popupai-app'"; exit 1; }
bin=target/debug/popupai

curl -sf http://127.0.0.1:11434/api/tags >/dev/null || { echo "Ollama is not running on 127.0.0.1:11434"; exit 1; }

cat >"$tmp/config.toml" <<'EOF'
default_provider = "ollama"

[[provider]]
id = "ollama"
kind = "ollama"
name = "Ollama"
EOF
chmod 600 "$tmp/config.toml"

log="$tmp/app.log"
POPUPAI_CONFIG="$tmp/config.toml" XDG_CONFIG_HOME="$tmp/xdg" QT_QPA_PLATFORM=wayland \
    QT_FORCE_STDERR_LOGGING=1 "$bin" --background >"$log" 2>&1 &
app=$!

# Give the portal time to answer (the desktop may show a confirmation dialog first).
for _ in $(seq 1 200); do
    grep -qE "global shortcut (active|unavailable|has no key)" "$log" && break
    sleep 0.1
done
# By default the popup is NOT shown by the script: the user opens it by clicking the tray
# icon (which also tests the icon), or with the shortcut once it has a key. A popup opened
# by the script right after the user's last action elsewhere was seen to lose the keyboard
# focus within about a second, because the window the user had just used took it back.
# AUTO_SHOW=1 restores the automatic opening for runs where nobody is at the keyboard.
if [ "${AUTO_SHOW:-0}" = "1" ]; then
    sleep 1
    "$bin" --toggle >/dev/null 2>&1 || true
else
    notify-send -a PopUpAI "PopUpAI test started" "Click the PopUpAI icon in the notification area to open the popup." 2>/dev/null || true
fi
sleep "$window"

echo
echo "=== what the application reported (metadata only) ==="
grep -E "provider selected|tray|global shortcut|shortcut settings|remote command|popup opened|request started|request finished|request failed|popup dismissed|ui event=(active|inactive)|WARN|ERROR" "$log" \
    | grep -vE "sfinae|proxy caching" || true
echo
echo "=== what KDE recorded for the shortcut (read-only) ==="
grep -A3 -F "[io.github.jeremiznoo.PopUpAI]" "$HOME/.config/kglobalshortcutsrc" 2>/dev/null || echo "(no entry)"
cp "$log" target/try-real.log
echo
echo "Full log kept in target/try-real.log"

"$bin" --quit >/dev/null 2>&1 || true
wait "$app" 2>/dev/null || true
app=""
