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
    icons_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons"
    (cd packaging/icons && find hicolor -type f) | while read -r icon; do rm -fv "$icons_dir/$icon"; done
    exit 0
fi

echo "Building the release binary..."
cargo build --release -p popupai-app
install -Dm755 target/release/popupai "$bin_dir/popupai"
# The launcher gets the full path of the binary: a desktop session does not always have ~/.local/bin in its PATH,
# and the shortcut that starts PopUpAI goes through this file.
install -Dm644 "packaging/$desktop" "$apps_dir/$desktop"
sed -i "s|^Exec=popupai |Exec=$bin_dir/popupai |" "$apps_dir/$desktop"
# The icon, in every size of the hicolor theme plus the scalable one.
icons_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons"
(cd packaging/icons && find hicolor -type f) | while read -r icon; do
    install -Dm644 "packaging/icons/$icon" "$icons_dir/$icon"
done
gtk-update-icon-cache -qtf "$icons_dir/hicolor" 2>/dev/null || true
echo "Installed:"
echo "  $bin_dir/popupai"
echo "  $apps_dir/$desktop"
echo "  the icon in $icons_dir/hicolor"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir is not in your PATH." ;;
esac
