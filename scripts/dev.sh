#!/usr/bin/env bash
# Runs build and test commands inside the Exegol "dev" container so nothing is
# installed on the host. The container is created once with the command in
# docs/DEVELOPMENT.md.
#
#   scripts/dev.sh setup            install the pinned Rust toolchain in the container
#   scripts/dev.sh cargo test       run any command in the container, as your uid
#   scripts/dev.sh shell            interactive shell in the container, as your uid
#   scripts/dev.sh clean            delete the build cache inside the container
set -euo pipefail

CONTAINER="exegol-dev"
TOOLCHAIN="1.99.1"
RUSTUP_HOME_DIR="/opt/rust/rustup"
RUST_BIN_DIR="/opt/rust/cargo/bin"
# Writable by any uid; lives inside the container, never in the project tree.
SCRATCH="/var/tmp/popupai"

die() {
    echo "error: $*" >&2
    exit 1
}

ensure_running() {
    local running
    running="$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" ||
        die "container '$CONTAINER' not found; create it as described in docs/DEVELOPMENT.md"
    [ "$running" = "true" ] || docker start "$CONTAINER" >/dev/null
}

# Run a command as the calling user (not root), so files written to the shared
# project directory stay owned by you.
as_user() {
    local tty_flags=(-i)
    [ -t 0 ] && [ -t 1 ] && tty_flags+=(-t)
    docker exec "${tty_flags[@]}" -u "$(id -u):$(id -g)" -w /workspace \
        -e HOME="$SCRATCH/home" \
        -e RUSTUP_HOME="$RUSTUP_HOME_DIR" \
        -e CARGO_HOME="$SCRATCH/cargo" \
        -e CARGO_TARGET_DIR="$SCRATCH/target" \
        -e PATH="$RUST_BIN_DIR:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
        "$CONTAINER" "$@"
}

setup() {
    ensure_running
    docker exec -i -u 0 \
        -e TOOLCHAIN="$TOOLCHAIN" -e SCRATCH="$SCRATCH" \
        -e RUSTUP_HOME="$RUSTUP_HOME_DIR" -e CARGO_HOME=/opt/rust/cargo \
        "$CONTAINER" bash -euo pipefail -s <<'EOF'
install -d -m 1777 "$SCRATCH" "$SCRATCH/home" "$SCRATCH/cargo" "$SCRATCH/target"

if /opt/rust/cargo/bin/rustc --version 2>/dev/null | grep -q " $TOOLCHAIN "; then
    echo "Rust $TOOLCHAIN already installed."
    exit 0
fi

base=https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu
dir="$(mktemp -d /tmp/rustup-dl.XXXXXX)"
trap 'rm -rf "$dir"' EXIT
cd "$dir"
curl -fsSL -o rustup-init "$base/rustup-init"
curl -fsSL -o rustup-init.sha256 "$base/rustup-init.sha256"
expected="$(cut -d' ' -f1 rustup-init.sha256)"
actual="$(sha256sum rustup-init | cut -d' ' -f1)"
echo "rustup-init sha256 expected: $expected"
echo "rustup-init sha256 actual:   $actual"
[ "$expected" = "$actual" ] || { echo "checksum mismatch, aborting" >&2; exit 1; }
chmod +x rustup-init
./rustup-init -y --no-modify-path --profile minimal \
    --default-toolchain "$TOOLCHAIN" -c rustfmt -c clippy
chmod -R a+rX /opt/rust
/opt/rust/cargo/bin/rustc --version
EOF
}

case "${1:-}" in
    setup)
        setup
        ;;
    shell)
        ensure_running
        as_user bash
        ;;
    clean)
        ensure_running
        docker exec -u 0 "$CONTAINER" find "$SCRATCH/target" "$SCRATCH/cargo" \
            -mindepth 1 -maxdepth 1 -exec rm -rf {} +
        ;;
    "" | -h | --help)
        sed -n '2,10p' "${BASH_SOURCE[0]}"
        ;;
    *)
        ensure_running
        as_user "$@"
        ;;
esac
