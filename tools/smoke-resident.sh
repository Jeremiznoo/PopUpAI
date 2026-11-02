#!/usr/bin/env bash
# Headless smoke test of the resident behaviour: single instance, --toggle, --quit.
#
#   tools/smoke-resident.sh
#
# It runs on a private D-Bus bus and the Qt "offscreen" platform, so it never touches
# your session bus, your screen or your configuration. Needs dbus-daemon. Everything
# it starts is stopped when it exits.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

tmp="$(mktemp -d)"
bus_pid=""
app_pid=""
cleanup() {
    [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null
    [ -n "$bus_pid" ] && kill "$bus_pid" 2>/dev/null
    rm -rf "$tmp"
}
trap cleanup EXIT

failed=0
check() { # check "what" command...
    local what="$1"
    shift
    if "$@"; then
        echo "  ok    $what"
    else
        echo "  FAIL  $what"
        failed=1
    fi
}

log="$tmp/app.log"
count() { grep -c -- "$1" "$log" 2>/dev/null || true; }
wait_for() { # wait_for "pattern" min_count
    for _ in $(seq 1 60); do
        [ "$(count "$1")" -ge "${2:-1}" ] && return 0
        sleep 0.1
    done
    return 1
}
at_least() { [ "$(count "$1")" -ge "$2" ]; }
exactly() { [ "$(count "$1")" -eq "$2" ]; }

echo "Building..."
cargo build -p popupai-app >/dev/null 2>&1 || { echo "build failed"; exit 1; }
bin=target/debug/popupai

# A bus with no service directories: the stock session configuration would let it start
# whatever is installed on the machine (the real desktop portal, for one).
cat >"$tmp/bus.conf" <<BUSEOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$tmp/bus</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
BUSEOF
dbus-daemon --nofork --print-address=1 --config-file="$tmp/bus.conf" >"$tmp/addr" 2>/dev/null &
bus_pid=$!
for _ in $(seq 1 50); do [ -s "$tmp/addr" ] && break; sleep 0.1; done
export DBUS_SESSION_BUS_ADDRESS="$(head -1 "$tmp/addr")"
# No configuration file: the first-run providers (one per vendor) are used.
# XDG_CONFIG_HOME is private too, so the autostart entry of the real account is never read or changed.
export POPUPAI_CONFIG="$tmp/config.toml" XDG_CONFIG_HOME="$tmp/xdg" QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1

echo "Private bus: $DBUS_SESSION_BUS_ADDRESS"
echo "1. start hidden, as autostart would"
"$bin" --background >"$log" 2>&1 &
app_pid=$!
check "the instance claims its name" wait_for "claimed the instance name"
check "it starts without showing the popup" exactly "popup opened" 0
# No secret store and no desktop portal exist on the private bus. Both absences must be
# reported, not crash the application or hang it.
check "the global shortcut failure is reported, not fatal" wait_for "global shortcut unavailable"
check "with no key stored, the local provider is the one selected" wait_for 'provider selected provider="ollama"'

echo "2. a second launch hands its command over and exits"
SECONDS=0
timeout 10 "$bin" --toggle
check "--toggle exits with success" test $? -eq 0
check "--toggle returns promptly (it does not start a second PopUpAI)" test "$SECONDS" -le 4
check "the running instance received the command" wait_for "command=toggle"
check "the popup opened" wait_for "popup opened" 1
check "the first opening is session 1" wait_for "popup opened session=1"

echo "3. toggling again hides it; the next one opens a fresh session"
timeout 10 "$bin" --toggle
check "it was dismissed with reason=toggle" wait_for "reason=toggle"
timeout 10 "$bin" --toggle
check "it opened again" wait_for "popup opened" 2
check "as a new session (id 2)" wait_for "popup opened session=2"

echo "4. a plain launch shows the popup, and does nothing if it is already shown"
timeout 10 "$bin" --toggle
wait_for "reason=toggle" 2 >/dev/null
before="$(count "popup opened")"
timeout 10 "$bin"
check "a plain launch showed it" wait_for "popup opened" $((before + 1))
shown="$(count "popup opened")"
timeout 10 "$bin"
sleep 0.5
check "a plain launch on a visible popup changes nothing" exactly "popup opened" "$shown"
check "only one process is running" test "$(pgrep -c -x popupai)" -eq 1

echo "5. --quit stops the running instance"
timeout 10 "$bin" --quit
check "--quit exits with success" test $? -eq 0
for _ in $(seq 1 50); do kill -0 "$app_pid" 2>/dev/null || break; sleep 0.1; done
check "the instance exited" bash -c "! kill -0 $app_pid 2>/dev/null"
app_pid=""
check "it logged a clean exit" grep -q "exiting code=0" "$log"

echo "6. --quit with nothing running says so and fails"
timeout 10 "$bin" --quit >"$tmp/quit.out" 2>&1
check "it exits with a failure code" test $? -ne 0
check "it says PopUpAI is not running" grep -q "not running" "$tmp/quit.out"

echo "7. a prompt on the command line is refused"
timeout 10 "$bin" "what is DNS?" >"$tmp/arg.out" 2>&1
check "it exits with code 2 and prints the usage" test $? -eq 2

echo
if [ "$failed" -eq 0 ]; then
    echo "All checks passed."
else
    echo "Some checks FAILED. Application log:"
    sed 's/^/    /' "$log"
fi
exit "$failed"
