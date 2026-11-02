import QtQuick
import QtQuick.Window
import org.kde.layershell 1.0 as LayerShell
import io.github.jeremiznoo.popupai

Window {
    id: popup

    // The application stays resident: dismissing hides the window and a later
    // command shows it again. Development aid: POPUPAI_EXIT_ON_DISMISS=1 quits
    // on dismissal instead.
    readonly property bool oneShot: controller.exitOnDismiss

    readonly property int maxHeight: 560
    property bool wasActive: false

    visible: false
    width: 440
    // The input stays at the bottom edge and the answer grows above it, so the field
    // does not move while an answer streams in.
    height: Math.min(maxHeight, content.implicitHeight + 24)
    color: "transparent"
    title: "PopUpAI"

    // Placement: bottom-right corner, above panels, keyboard focus on demand
    // (ADR 0002, spikes S1 to S3).
    LayerShell.Window.scope: "popupai"
    LayerShell.Window.layer: LayerShell.Window.LayerOverlay
    LayerShell.Window.anchors: LayerShell.Window.AnchorBottom | LayerShell.Window.AnchorRight
    LayerShell.Window.margins.right: 16
    LayerShell.Window.margins.bottom: 16
    LayerShell.Window.exclusionZone: 0
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityOnDemand

    SystemPalette { id: palette }
    PopupController { id: controller }

    // Used only to write the answer to the clipboard when the user asks for it.
    TextEdit { id: clipboard; visible: false }

    function focusField() {
        if (controller.needsKey)
            keyInput.forceActiveFocus()
        else
            input.forceActiveFocus()
    }

    function openPopup() {
        controller.open()
        input.text = ""
        keyInput.text = ""
        wasActive = false
        visible = true
        focusField()
    }

    function closePopup(reason) {
        controller.dismiss(reason)
        visible = false
        wasActive = false
        keyInput.text = ""
        if (oneShot)
            Qt.quit()
    }

    function submit() {
        if (input.text.trim().length === 0)
            return
        controller.submit(input.text)
        input.text = ""
    }

    function saveKey() {
        controller.saveKey(keyInput.text)
        // The pasted key leaves this field at once; the controller wipes its copy.
        keyInput.text = ""
    }

    function copyAnswer() {
        clipboard.text = controller.answer
        clipboard.selectAll()
        clipboard.copy()
    }

    // Outside-click dismissal is "the window lost keyboard focus". Ignore the
    // deactivation that cannot happen before the window has been active once.
    onActiveChanged: {
        controller.trace(active ? "active" : "inactive")
        if (active)
            wasActive = true
        else if (wasActive && visible)
            closePopup("focus lost")
    }

    // Timing events for the log (fixed labels only), to measure how fast the popup
    // appears once the application is warm.
    property bool awaitingFirstFrame: false

    onVisibleChanged: {
        if (visible) {
            awaitingFirstFrame = true
            controller.trace("visible")
        }
    }

    onFrameSwapped: {
        if (awaitingFirstFrame) {
            awaitingFirstFrame = false
            controller.trace("first frame")
        }
    }

    Component.onCompleted: {
        if (controller.startVisible)
            openPopup()
    }

    // Window-wide keys, whichever field has the focus.
    Shortcut { sequence: "Esc"; onActivated: popup.closePopup("escape") }
    Shortcut { sequence: "Ctrl+P"; onActivated: controller.cycleProvider() }
    Shortcut { sequence: "Ctrl+R"; onActivated: controller.resume() }
    Shortcut { sequence: "Ctrl+."; onActivated: controller.stop() }
    Shortcut {
        sequence: "Ctrl+Shift+C"
        enabled: controller.answer.length > 0
        onActivated: popup.copyAnswer()
    }

    Connections {
        target: controller

        // Commands from later invocations (`popupai --toggle`, `--quit`) and from the
        // global shortcut.
        function onCommand(name) {
            if (name === "toggle") {
                if (popup.visible)
                    popup.closePopup("toggle")
                else
                    popup.openPopup()
            } else if (name === "show") {
                if (!popup.visible)
                    popup.openPopup()
            } else if (name === "hide") {
                if (popup.visible)
                    popup.closePopup("hide")
            } else if (name === "quit") {
                Qt.quit()
            }
        }

        // The field to type in changes when a key turns out to be missing or has
        // just been saved.
        function onNeedsKeyChanged() {
            if (popup.visible)
                popup.focusField()
        }
    }

    component TextButton: Text {
        id: button
        signal clicked()

        font.pixelSize: 11
        color: Qt.alpha(palette.windowText, area.containsMouse ? 0.9 : 0.55)
        font.underline: area.containsMouse

        MouseArea {
            id: area
            anchors.fill: parent
            anchors.margins: -4
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: button.clicked()
        }
    }

    Rectangle {
        id: panel
        anchors.fill: parent
        radius: 12
        color: palette.window
        border.color: Qt.alpha(palette.windowText, 0.18)
        border.width: 1

        Column {
            id: content
            anchors {
                left: parent.left
                right: parent.right
                bottom: parent.bottom
                margins: 12
            }
            spacing: 8

            // A problem with the settings or the global shortcut, only when there is one.
            Text {
                visible: controller.notice.length > 0
                width: parent.width
                wrapMode: Text.Wrap
                text: controller.notice
                font.pixelSize: 11
                color: "#bf616a"
            }

            Text {
                visible: controller.question.length > 0
                width: parent.width
                elide: Text.ElideRight
                maximumLineCount: 1
                text: controller.question
                font.pixelSize: 12
                color: Qt.alpha(palette.windowText, 0.6)
            }

            Text {
                id: thinking
                visible: controller.status === "streaming" && controller.answer.length === 0
                text: "…"
                font.pixelSize: 16
                color: Qt.alpha(palette.windowText, 0.6)

                SequentialAnimation on opacity {
                    running: thinking.visible
                    loops: Animation.Infinite
                    NumberAnimation { from: 1.0; to: 0.25; duration: 600 }
                    NumberAnimation { from: 0.25; to: 1.0; duration: 600 }
                }
            }

            Flickable {
                id: answerView
                visible: controller.answer.length > 0
                width: parent.width
                height: Math.min(answerText.implicitHeight, popup.maxHeight - 120)
                contentHeight: answerText.implicitHeight
                clip: true
                interactive: contentHeight > height

                // The answer is Markdown that the core has already reduced to a small,
                // attribute-free HTML subset: no image, no link, no raw HTML reaches
                // this widget (see popupai-core `markdown`).
                TextEdit {
                    id: answerText
                    width: answerView.width
                    readOnly: true
                    activeFocusOnPress: false  // the question field keeps the keyboard
                    selectByMouse: true
                    wrapMode: TextEdit.Wrap
                    textFormat: TextEdit.RichText
                    text: controller.answerHtml
                    font.pixelSize: 13
                    color: palette.windowText
                    selectionColor: palette.highlight
                    selectedTextColor: palette.highlightedText
                }

                onContentHeightChanged: {
                    if (controller.status === "streaming")
                        contentY = Math.max(0, contentHeight - height)
                }
            }

            Text {
                visible: controller.errorText.length > 0
                width: parent.width
                wrapMode: Text.Wrap
                text: controller.errorText
                font.pixelSize: 12
                color: "#bf616a"
            }

            Text {
                visible: controller.backgroundAnswer && controller.status === "idle"
                width: parent.width
                text: "A previous answer is ready · Ctrl+R"
                font.pixelSize: 11
                color: Qt.alpha(palette.windowText, 0.6)
            }

            // The one thing that is always there: the field.
            Rectangle {
                width: parent.width
                height: Math.min(120, Math.max(38, (controller.needsKey ? keyInput.implicitHeight : input.contentHeight) + 18))
                radius: 8
                color: palette.base
                border.color: (input.activeFocus || keyInput.activeFocus)
                    ? palette.highlight : Qt.alpha(palette.windowText, 0.2)

                TextEdit {
                    id: input
                    visible: !controller.needsKey
                    anchors.fill: parent
                    anchors.margins: 9
                    color: palette.text
                    font.pixelSize: 14
                    wrapMode: TextEdit.Wrap
                    selectByMouse: true
                    focus: true

                    Text {
                        visible: input.text.length === 0 && input.preeditText.length === 0
                        text: "Ask…"
                        font: input.font
                        color: Qt.alpha(palette.text, 0.4)
                    }

                    Keys.onPressed: event => {
                        if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                                && !(event.modifiers & Qt.ShiftModifier)) {
                            popup.submit()
                            event.accepted = true
                        }
                    }
                }

                // Shown instead of the question field when the provider needs an API key
                // that is not stored yet. Masked, and handed straight to the secret store.
                TextInput {
                    id: keyInput
                    visible: controller.needsKey
                    anchors.fill: parent
                    anchors.margins: 9
                    verticalAlignment: TextInput.AlignVCenter
                    echoMode: TextInput.Password
                    inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
                    color: palette.text
                    font.pixelSize: 14
                    selectByMouse: true

                    Text {
                        visible: keyInput.text.length === 0
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Paste your API key, then Enter"
                        font: keyInput.font
                        color: Qt.alpha(palette.text, 0.4)
                    }

                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            popup.saveKey()
                            event.accepted = true
                        }
                    }
                }
            }

            // Which model answers, and the few actions that apply right now.
            Item {
                width: parent.width
                height: 14

                TextButton {
                    anchors.left: parent.left
                    anchors.right: actions.left
                    anchors.rightMargin: 8
                    elide: Text.ElideRight
                    text: controller.needsKey
                        ? controller.providerLabel + " · key goes to the system wallet"
                        : controller.providerLabel
                    onClicked: controller.cycleProvider()
                }

                Row {
                    id: actions
                    anchors.right: parent.right
                    spacing: 12

                    TextButton {
                        visible: controller.status === "streaming"
                        text: "Stop"
                        onClicked: controller.stop()
                    }
                    TextButton {
                        visible: controller.answer.length > 0
                        text: "Copy"
                        onClicked: popup.copyAnswer()
                    }
                }
            }
        }
    }
}
