#!/usr/bin/env bash
# Install PopUpAI for the current user only (no root): the binary in ~/.local/bin and the
# launcher entry in ~/.local/share/applications. Nothing else is touched.
#
#   tools/install-local.sh            install
#   tools/install-local.sh --remove   remove what this script installed
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
desktop="io.github.jeremiznoo.PopUpAI.desktop"

if [ "${1:-}" = "--remove" ]; then
    rm -fv "$bin_dir/popupai" "$apps_dir/$desktop"
    exit 0
fi

echo "Building the release binary..."
cargo build --release -p popupai-app
install -Dm755 target/release/popupai "$bin_dir/popupai"
install -Dm644 "packaging/$desktop" "$apps_dir/$desktop"
echo "Installed:"
echo "  $bin_dir/popupai"
echo "  $apps_dir/$desktop"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir is not in your PATH." ;;
esac
