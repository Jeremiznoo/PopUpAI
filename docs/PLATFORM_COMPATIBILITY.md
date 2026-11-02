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
| Corner placement | ✅ bottom-right corner, readable; confirmed by eye by the user with the spike and with the real popup (reported, not measured; one monitor) | ⬜ via `xcb`; dev machine can only test through XWayland | ❌ Mutter is not known to implement layer-shell **(assumption)** | ⬜ | ⬜ |
| Initial keyboard focus | ✅ three runs: active 46 to 56 ms after show with no click; 36 typed keys delivered with no mouse press (one monitor, `OnDemand`); the real resident popup also became active after 18 of 18 shows, re-shows of a recreated surface included, and an unattended run showed no spontaneous focus loss over 6 shows | ⬜ | ⬜ | ⬜ | ⬜ |
| Outside-click dismissal | ✅ the window was deactivated by the user's outside click (run 3, logged), and the user confirmed that single click also worked in the window underneath (reported, not measured by the program) | ⬜ | ⬜ | ⬜ | ⬜ |
| System tray | 🟡 StatusNotifierWatcher present | ⬜ | ⬜ needs an extension **(assumption)** | ⬜ | ⬜ |
| Secret storage | 🟡 `ksecretd` serves `org.freedesktop.secrets` | ⬜ | ⬜ | ⬜ Credential Manager | ⬜ Keychain |
| Autostart | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Multi-monitor and mixed scaling | ⬜ dev machine has one output | ⬜ | ⬜ | ⬜ | ⬜ |

## Spike log

Observations from [spikes/s1-layer-shell](../spikes/s1-layer-shell/README.md), on KDE
Plasma 6.8.2 Wayland, Qt 6.11.2, LayerShellQt 6.8.2, one monitor. Three runs so far.
The on-screen placement was confirmed by eye by the user (bottom-right corner,
readable); that is a report, not a measurement. Multi-monitor (S6) is untested.
`Shell::useLayerShell()` is deprecated since Qt 6.6 and is not needed.

## Notes

- **Wayland compatibility is not claimed.** Successful compilation will not
  change that; only the spikes in [ADR 0002](ADR/0002-linux-desktop-integration.md) do.
- **GNOME** is not an MVP target. Without layer-shell the popup cannot be pinned
  to a corner; if GNOME support is added it will be documented as a degraded mode
  (no positioning guarantee), never silently.
- **Windows and macOS** are architecture-only for now. The abstractions in
  [ARCHITECTURE.md](ARCHITECTURE.md) are designed so they can be added without
  touching `popupai-core`, `popupai-ai` or `popupai-security`.
