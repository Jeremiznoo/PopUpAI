# Platform compatibility

Last updated: 2026-11-02. This table records what was **observed on the real session**,
what is implemented but only **tested against mocks**, and what is only *planned*.
A cell becomes a check mark only after the matching test has been run on that
platform and the result is noted here.

Legend: ✅ tested and working · 🟡 prerequisite verified, feature not yet
exercised · ⬜ planned, not started · ❌ not supported.

| Capability | KDE Plasma 6 Wayland | KDE X11 | GNOME Wayland | Windows 10/11 | macOS |
|---|---|---|---|---|---|
| Global shortcut | 🟡 **Observed on the real session (2026-11-02, from the application's own logs and `kglobalshortcutsrc`):** the portal refuses the registration ("App info not found") until a launcher file named after the application id is installed; once it is, KDE accepts the binding, records the requested trigger as the **default** and leaves the **active key unassigned** until the user assigns it (in KDE's shortcut window, opened by the popup's *Set the shortcut…* button through `ConfigureShortcuts`). The user then reported that the shortcut and the popup work; that is a report, the `Activated` signal was not captured in my logs. Also covered by 12 end-to-end tests against a mock portal | ⬜ | ⬜ portal exists on GNOME 48+ **(assumption)** | ⬜ | ⬜ |
| Corner placement | ✅ bottom-right corner, readable; confirmed by eye by the user with the spike and with the real popup (reported, not measured; one monitor) | ⬜ via `xcb`; dev machine can only test through XWayland | ❌ Mutter is not known to implement layer-shell **(assumption)** | ⬜ | ⬜ |
| Initial keyboard focus | ✅ three runs: active 46 to 56 ms after show with no click; 36 typed keys delivered with no mouse press (one monitor, `OnDemand`); the real resident popup also became active after 18 of 18 shows, re-shows of a recreated surface included, and an unattended run showed no spontaneous focus loss over 6 shows | ⬜ | ⬜ | ⬜ | ⬜ |
| Outside-click dismissal | ✅ the window was deactivated by the user's outside click (run 3, logged), and the user confirmed that single click also worked in the window underneath (reported, not measured by the program) | ⬜ | ⬜ | ⬜ | ⬜ |
| System tray | 🟡 the icon registers with the real StatusNotifier host without error (logged on every real run); the user reported that it works (a report, not measured). Also tested against a mock host (registers, answers Activate) | ⬜ | ⬜ needs an extension **(assumption)** | ⬜ | ⬜ |
| Secret storage | 🟡 Secret Service client implemented, fails closed without a service; `ksecretd` serves `org.freedesktop.secrets` here; **the real round trip is an ignored test that was NOT run: the user did not authorise touching the wallet**, so no key has been stored or read through it yet | ⬜ | ⬜ | ⬜ Credential Manager | ⬜ Keychain |
| Autostart | 🟡 XDG autostart entry written and removed (tested on a temporary folder); **not verified that Plasma starts it at login** | 🟡 same | 🟡 same | ⬜ | ⬜ |
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
