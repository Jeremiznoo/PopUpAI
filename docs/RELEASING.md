# Releasing

## What is ready

- `LICENSE` (MIT) and the `license` field of every crate.
- The Fedora spec, the Debian packaging, the AUR `PKGBUILD` and the release workflow (see `release/README.md`).

## What has been done

- **Fedora `.rpm`:** built by the release workflow in a Fedora container when a `v*` tag is pushed, and attached
  to a pre-release. The build works (the spec, the dependencies and the icon and launcher install). The package
  has not been installed or run on a Fedora system other than the development machine.
- **Debian `.deb`:** built with `tools/build-deb.sh` in a Debian 13 container made from the Exegol image, then
  attached to the release with `gh release upload`. It installs on Debian 13 with its dependencies resolved, and
  starts there (headless): installing it showed that Debian does not package the `org.kde.layershell` QML
  module, so the interface now falls back to a plain window without it. It has not been run on a real Debian
  desktop, nor on Ubuntu.

A package built on one distribution would not run on another (Qt and libc differ), which is why each is built
in a container of its target.

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
