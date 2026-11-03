# Release

The packages are built by CI, in the distribution each one targets, when a version tag is pushed
(`git tag v0.1.0 && git push origin v0.1.0`): see [.github/workflows/release.yml](../.github/workflows/release.yml).
They are attached to the GitHub release; binaries are not committed to the repository.

| Target | Source | Built by |
|---|---|---|
| Fedora `.rpm` | [packaging/rpm/popupai.spec](../packaging/rpm/popupai.spec) | `fedora:latest` container |
| Debian / Ubuntu `.deb` | [packaging/debian/](../packaging/debian/) | `debian:trixie` container |
| Arch (AUR) | [packaging/aur/PKGBUILD](../packaging/aur/PKGBUILD) | by the AUR maintainer, with `makepkg` |

Nothing here has been built yet: see [docs/RELEASING.md](../docs/RELEASING.md).
