import org.kde.layershell 1.0 as LayerShell

// The popup pinned to the bottom-right corner, over panels, with keyboard focus on demand
// (ADR 0002, spikes S1 to S3). It needs the `org.kde.layershell` QML module, which some
// distributions do not package: Main.qml falls back to PopupWindow when it is missing.
PopupWindow {
    LayerShell.Window.scope: "popupai"
    LayerShell.Window.layer: LayerShell.Window.LayerOverlay
    LayerShell.Window.anchors: LayerShell.Window.AnchorBottom | LayerShell.Window.AnchorRight
    LayerShell.Window.margins.right: 4
    LayerShell.Window.margins.bottom: 4
    LayerShell.Window.exclusionZone: 0
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityOnDemand
}
