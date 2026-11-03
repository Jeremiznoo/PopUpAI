# Releasing

## What is ready

- `LICENSE` (MIT) and the `license` field of every crate.
- The Fedora spec, the Debian packaging, the AUR `PKGBUILD` and the release workflow (see `release/README.md`).

## What has not been done

None of the packages has been built, installed or run. The Fedora spec and the Debian control file list
dependencies written by hand: the first CI build is their first test, and the Debian runtime
dependencies in particular (the `qml6-module-*` packages and `layer-shell-qt`) have to be confirmed on a clean
Debian or Ubuntu system. The workflow uses a pre-release flag on purpose.

A `.deb` or `.rpm` built on the development machine would not run elsewhere: Qt and libc differ between
distributions. That is why the packages are built by CI in a container of each target.

## Before making the repository public

- Run the application on the real desktops listed in the README, not only KDE Plasma.
- Try a real ChatGPT tool call and a real MCP server (see `docs/PLATFORM_COMPATIBILITY.md`).
- Confirm the shortcut setup on each desktop it claims to support.
- Decide the name of the copyright holder in `LICENSE` (it currently says `jeremiznoo`).
- Make the repository public, then tag a version.

## AUR

The AUR package builds from the tag's tarball, so it needs a public repository. Once public, the maintainer
copies `packaging/aur/PKGBUILD` to a clone of `ssh://aur@aur.archlinux.org/popupai.git`, runs
`makepkg --printsrcinfo > .SRCINFO`, tests with `makepkg -si` on Arch, and pushes.
