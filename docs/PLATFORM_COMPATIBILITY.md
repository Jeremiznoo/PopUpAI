# Platform compatibility

Last updated: 2026-11-02. **Nothing has been exercised yet.** This table records
what the environment has been verified to *offer* and what is only *planned*.
A cell becomes a check mark only after the matching test has been run on that
platform and the result is noted here.

Legend: ✅ tested and working · 🟡 prerequisite verified, feature not yet
exercised · ⬜ planned, not started · ❌ not supported.

| Capability | KDE Plasma 6 Wayland | KDE X11 | GNOME Wayland | Windows 10/11 | macOS |
|---|---|---|---|---|---|
| Global shortcut | 🟡 portal v2 present (KDE backend) | ⬜ | ⬜ portal exists on GNOME 48+ **(assumption)** | ⬜ | ⬜ |
| Corner placement | 🟡 `wlr-layer-shell` v5 advertised | ⬜ via `xcb`; dev machine can only test through XWayland | ❌ Mutter is not known to implement layer-shell **(assumption)** | ⬜ | ⬜ |
| Initial keyboard focus | ⬜ open question (spike S2) | ⬜ | ⬜ | ⬜ | ⬜ |
| Outside-click dismissal | ⬜ via focus loss (spike S3) | ⬜ | ⬜ | ⬜ | ⬜ |
| System tray | 🟡 StatusNotifierWatcher present | ⬜ | ⬜ needs an extension **(assumption)** | ⬜ | ⬜ |
| Secret storage | 🟡 `ksecretd` serves `org.freedesktop.secrets` | ⬜ | ⬜ | ⬜ Credential Manager | ⬜ Keychain |
| Autostart | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Multi-monitor and mixed scaling | ⬜ dev machine has one output | ⬜ | ⬜ | ⬜ | ⬜ |

## Notes

- **Wayland compatibility is not claimed.** Successful compilation will not
  change that; only the spikes in [ADR 0002](ADR/0002-linux-desktop-integration.md) do.
- **GNOME** is not an MVP target. Without layer-shell the popup cannot be pinned
  to a corner; if GNOME support is added it will be documented as a degraded mode
  (no positioning guarantee), never silently.
- **Windows and macOS** are architecture-only for now. The abstractions in
  [ARCHITECTURE.md](ARCHITECTURE.md) are designed so they can be added without
  touching `popupai-core`, `popupai-ai` or `popupai-security`.
