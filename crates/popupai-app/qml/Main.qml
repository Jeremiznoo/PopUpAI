import QtQuick

// Chooses the window: pinned to the corner through layer-shell when the QML module for it is
// installed (KDE Plasma, sway, Hyprland...), a plain frameless window otherwise (X11, GNOME...).
QtObject {
    id: root
    property var window

    Component.onCompleted: {
        let component = Qt.createComponent("LayerPopupWindow.qml")
        if (component.status !== Component.Ready) {
            console.warn("layer-shell is not available; using a plain window")
            component = Qt.createComponent("PopupWindow.qml")
        }
        if (component.status !== Component.Ready) {
            console.error("the popup window could not be created: " + component.errorString())
            Qt.exit(1)
            return
        }
        window = component.createObject(root)
    }
}
