#!/usr/bin/env bash
# Build the Debian package in a Debian 13 (trixie) container made from the Exegol image, and leave it in release/.
#
#   tools/build-deb.sh [version]        (default: the version in Cargo.toml)
#
# Why not build in the Exegol container directly: its image is Debian 12, whose Qt (6.4) is too old for the
# interface. So a trixie root is created inside an Exegol-image container with debootstrap, turned into a local
# image (`popupai-trixie`, reused by later runs), and the package is built there. Needs Docker, the
# `nwodtuhs/exegol:free` image, network access, and a few GB of disk. The first run takes a while (download and
# compile); later runs only compile.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

version="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
image="popupai-trixie"
mkdir -p release

if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "Creating the Debian 13 build image (once)..."
    docker rm -f popupai-trixie-setup >/dev/null 2>&1 || true
    docker run --name popupai-trixie-setup --entrypoint bash nwodtuhs/exegol:free -c '
        set -euxo pipefail
        export DEBIAN_FRONTEND=noninteractive
        apt-get update && apt-get install -y --no-install-recommends debootstrap ca-certificates
        debootstrap --variant=minbase trixie /trixie http://deb.debian.org/debian
        cp /etc/resolv.conf /trixie/etc/resolv.conf
        chroot /trixie /bin/bash -euxo pipefail -c "
            export DEBIAN_FRONTEND=noninteractive
            apt-get update
            apt-get install -y --no-install-recommends build-essential cmake pkg-config curl ca-certificates \
                debhelper fakeroot dpkg-dev qt6-base-dev qt6-declarative-dev libgl-dev git"
    '
    docker cp popupai-trixie-setup:/trixie/. - | docker import - "$image"
    docker rm -f popupai-trixie-setup >/dev/null
fi

docker run --rm -e VERSION="$version" -v "$PWD":/src:ro -v "$PWD/release":/out "$image" /bin/bash -euxo pipefail -c '
    export DEBIAN_FRONTEND=noninteractive HOME=/root
    rm -rf /build && mkdir -p /build
    tar -C /src --exclude=./target --exclude=./.git --exclude=./release -cf - . | tar -C /build -xf -
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
    export PATH=$HOME/.cargo/bin:$PATH
    cd /build
    cp -r packaging/debian debian
    sed -i "1s/.*/popupai (${VERSION}) unstable; urgency=medium/" debian/changelog
    dpkg-buildpackage -us -uc -b
    cp /popupai_*.deb /out/
    chown -R '"$(id -u):$(id -g)"' /out
'
ls -la release/*.deb
