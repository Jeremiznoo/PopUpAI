# Environment report (Phase 0)

Audited on 2026-11-02 on the primary development machine. Every value below was
read from the live system; nothing is inferred unless marked **(assumption)**.

## Summary

The desktop is a good fit for the product's primary target: KDE Plasma 6.8 on
Wayland, with the XDG GlobalShortcuts portal, `wlr-layer-shell`, a Secret
Service and a StatusNotifier host all present. The development toolchain is
**not** installed on the host (no Rust, no C++ compiler, no Qt development
files). By decision of the project owner the host stays untouched: builds and
tests run in a Docker container, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Operating system and desktop

| Item | Value |
|---|---|
| Distribution | Fedora Linux 44 (KDE Plasma Desktop Edition) |
| Kernel | 7.2.14-200.fc44.x86_64 |
| Plasma / KWin | 6.8.2 / 6.8.2 (`kwin_wayland`) |
| KDE Frameworks | 6.31.0 |
| Qt runtime | 6.11.2 (qtbase, qtdeclarative, qtwayland installed; no `-devel`) |
| Session | `XDG_SESSION_TYPE=wayland`, `WAYLAND_DISPLAY=wayland-1`, XWayland on `DISPLAY=:0` |
| Display | 1 output: eDP-1, 1920x1200 @ 60 Hz, scale 1 |
| CPU / RAM | 8 threads / 30 GiB |
| GPU | Intel Panther Lake integrated graphics, no discrete GPU |
| Disk | 189 GiB free on `/home` |

## Desktop integration surface

| Capability | Finding | How verified |
|---|---|---|
| Portals | `xdg-desktop-portal` 1.22.1, `-kde` 6.8.2, `-gtk` 1.15.3 running | `rpm -qa`, `ps` |
| GlobalShortcuts | Interface present, **version 2**, implemented by the KDE backend | `busctl --user introspect` / `get-property` |
| Secret portal | Routed to `kwallet` (`kde-portals.conf`) | config file |
| Secret Service | `org.freedesktop.secrets` owned by `ksecretd`; `kwalletd6` also running | `busctl --user list` |
| Tray | `org.kde.StatusNotifierWatcher` owned by `kded6` | `busctl --user list` |
| KGlobalAccel | `org.kde.kglobalaccel` served by KWin | `busctl --user list` |
| KWin scripting | `org.kde.KWin /Scripting` reachable (**not used**, see ADR 0002) | `busctl --user introspect` |

Wayland globals advertised by KWin (read-only registry probe), relevant subset:
`zwlr_layer_shell_v1` v5, `xdg_wm_base` v6, `xdg_activation_v1` v1,
`org_kde_kwin_shadow_manager` v2, `ext_background_effect_manager_v1` v1,
`wp_fractional_scale_manager_v1`, `wp_viewporter`, `ext_idle_notifier_v1` v2,
`zwp_keyboard_shortcuts_inhibit_manager_v1`, `org_kde_plasma_shell` v8.

**What this does and does not prove:** the protocols and services are
*available*. Nothing has been *exercised* yet: whether KWin hands keyboard focus
to an on-demand layer surface when it is mapped, and how the portal behaves for
an unsandboxed binary, are open questions (see ADR 0002, spikes S1 to S5).

## Development toolchain

| Present | Missing |
|---|---|
| `git` 2.56.0, `gh` 2.97.0, `gcc` 16.2.1 (C only), `pkg-config`, `node` 22.23.1, `npm`, `python3` 3.14.8, `flatpak` 1.18.4, `busctl`, `gdbus`, `qdbus-qt6`, `kscreen-doctor`, `wl-clipboard`, `spectacle`, `xdotool` (XWayland only) | `rustc`, `cargo`, `rustup`, `g++`, `cmake`, `ninja`, `clang`, `qmake6`, Qt `-devel`, `layer-shell-qt-devel`, `rpmbuild`, `flatpak-builder`, `appimagetool` |

`sudo` requires a password, so an automated agent cannot install system packages.

## Local AI

`/usr/local/bin/ollama` is installed. Its systemd unit is enabled but was in the
`activating` state and nothing answered on `127.0.0.1:11434` during the audit.
Nothing was started or downloaded. With integrated graphics only (no CUDA or
discrete VRAM), local inference will be CPU / iGPU bound; model recommendations
will be measured on this machine rather than assumed.

## Limits of this machine for testing

- **Single monitor.** Multi-monitor behaviour cannot be verified here. It needs a
  second output or a nested KWin session, and is tracked as a manual test.
- **No native X11 session.** The X11 path can only be exercised through XWayland
  (`QT_QPA_PLATFORM=xcb`), which is not equivalent to a real X11 desktop.
- **Windows and macOS** are not available.

## Repository status at audit time

`/home/fedora/Projets/PopUpAI` was empty and not a Git repository. It was
initialised with `git init -b main`. No user files existed, so nothing was
preserved or overwritten. The global Git configuration has an email but no
`user.name`; see the open question in the project hand-off.
