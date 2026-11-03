# Release

The Fedora package is built by CI, in a Fedora container, when a version tag is pushed
(`git tag v0.1.0 && git push origin v0.1.0`): see [.github/workflows/release.yml](../.github/workflows/release.yml).
It is attached to the GitHub pre-release. The Debian package is built with `tools/build-deb.sh` (a Debian 13
container made from the Exegol image) and added to the release with `gh release upload`. Binaries are not committed
to the repository.

| Target | Source | Built by |
|---|---|---|
| Fedora `.rpm` | [packaging/rpm/popupai.spec](../packaging/rpm/popupai.spec) | CI, `fedora:latest` container |
| Debian `.deb` | [packaging/debian/](../packaging/debian/) | by hand, in a Debian container |
| Arch (AUR) | [packaging/aur/PKGBUILD](../packaging/aur/PKGBUILD) | by the AUR maintainer, with `makepkg` |

