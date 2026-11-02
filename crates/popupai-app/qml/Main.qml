import QtQuick
import QtQuick.Window
import org.kde.layershell 1.0 as LayerShell
import io.github.popupai

Window {
    id: popup

    // The application stays resident: dismissing hides the window and a later
    // command shows it again. Development aid: POPUPAI_EXIT_ON_DISMISS=1 quits
    // on dismissal instead.
    readonly property bool oneShot: controller.exitOnDismiss

    readonly property int minHeight: 200
    readonly property int maxHeight: 600
    property bool wasActive: false

    visible: false
    width: 480
    height: Math.max(minHeight, Math.min(maxHeight, content.implicitHeight + 32))
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

    function openPopup() {
        controller.open()
        input.text = ""
        wasActive = false
        visible = true
        input.forceActiveFocus()
    }

    function closePopup(reason) {
        controller.dismiss(reason)
        visible = false
        wasActive = false
        if (oneShot)
            Qt.quit()
    }

    function submit() {
        if (input.text.trim().length === 0)
            return
        controller.submit(input.text)
        input.text = ""
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

    // Commands from later invocations (`popupai --toggle`, `--quit`).
    Connections {
        target: controller

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
    }

    component PillButton: Rectangle {
        id: pill
        property alias text: label.text
        signal clicked()

        implicitWidth: label.implicitWidth + 20
        implicitHeight: 24
        radius: 12
        color: area.containsMouse ? Qt.alpha(palette.highlight, 0.25) : Qt.alpha(palette.windowText, 0.08)

        Text {
            id: label
            anchors.centerIn: parent
            font.pixelSize: 12
            color: palette.windowText
        }
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            onClicked: pill.clicked()
        }
    }

    Rectangle {
        id: panel
        anchors.fill: parent
        radius: 14
        color: palette.window
        border.color: Qt.alpha(palette.windowText, 0.2)
        border.width: 1

        Column {
            id: content
            anchors {
                left: parent.left
                right: parent.right
                top: parent.top
                margins: 16
            }
            spacing: 10

            Item {
                width: parent.width
                height: brand.implicitHeight

                Text {
                    id: brand
                    text: "PopUpAI"
                    font.bold: true
                    font.pixelSize: 13
                    color: palette.windowText
                }
                Text {
                    anchors.left: brand.right
                    anchors.leftMargin: 10
                    anchors.right: parent.right
                    horizontalAlignment: Text.AlignRight
                    elide: Text.ElideRight
                    text: controller.providerLabel
                    font.pixelSize: 12
                    color: Qt.alpha(palette.windowText, 0.6)
                }
            }

            Text {
                visible: controller.notice.length > 0
                width: parent.width
                wrapMode: Text.Wrap
                text: controller.notice
                font.pixelSize: 12
                color: "#bf616a"
            }

            Text {
                visible: controller.backgroundAnswer && controller.status === "idle"
                width: parent.width
                text: "A previous answer is ready. Press Ctrl+R to see it."
                font.pixelSize: 12
                color: Qt.alpha(palette.windowText, 0.7)
            }

            Rectangle {
                width: parent.width
                height: Math.min(120, Math.max(40, input.contentHeight + 16))
                radius: 8
                color: palette.base
                border.color: input.activeFocus ? palette.highlight : Qt.alpha(palette.windowText, 0.25)

                TextEdit {
                    id: input
                    anchors.fill: parent
                    anchors.margins: 8
                    color: palette.text
                    font.pixelSize: 14
                    wrapMode: TextEdit.Wrap
                    selectByMouse: true
                    focus: true

                    Text {
                        visible: input.text.length === 0 && input.preeditText.length === 0
                        text: "Ask anything…"
                        font: input.font
                        color: Qt.alpha(palette.text, 0.45)
                    }

                    Keys.onPressed: event => {
                        const ctrl = (event.modifiers & Qt.ControlModifier) !== 0
                        if (event.key === Qt.Key_Escape) {
                            popup.closePopup("escape")
                            event.accepted = true
                        } else if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                                   && !(event.modifiers & Qt.ShiftModifier)) {
                            popup.submit()
                            event.accepted = true
                        } else if (ctrl && event.key === Qt.Key_R) {
                            controller.resume()
                            event.accepted = true
                        } else if (ctrl && event.key === Qt.Key_Period) {
                            controller.stop()
                            event.accepted = true
                        }
                    }
                }
            }

            Text {
                visible: controller.question.length > 0
                width: parent.width
                wrapMode: Text.Wrap
                text: controller.question
                font.pixelSize: 13
                font.bold: true
                color: palette.windowText
            }

            Text {
                id: thinking
                visible: controller.status === "streaming" && controller.answer.length === 0
                text: "Thinking…"
                font.pixelSize: 13
                color: Qt.alpha(palette.windowText, 0.6)

                SequentialAnimation on opacity {
                    running: thinking.visible
                    loops: Animation.Infinite
                    NumberAnimation { from: 1.0; to: 0.3; duration: 700 }
                    NumberAnimation { from: 0.3; to: 1.0; duration: 700 }
                }
            }

            Flickable {
                id: answerView
                visible: controller.answer.length > 0
                width: parent.width
                height: Math.min(answerText.implicitHeight, 300)
                contentHeight: answerText.implicitHeight
                clip: true
                interactive: contentHeight > height

                Text {
                    id: answerText
                    width: answerView.width
                    wrapMode: Text.Wrap
                    // Plain text on purpose: model output is untrusted, and rich
                    // text could load remote images. Markdown rendering comes with
                    // a renderer that refuses them.
                    textFormat: Text.PlainText
                    text: controller.answer
                    font.pixelSize: 13
                    color: palette.windowText
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

            Item {
                width: parent.width
                height: 24

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Enter send · Shift+Enter newline · Esc close"
                    font.pixelSize: 11
                    color: Qt.alpha(palette.windowText, 0.5)
                }
                Row {
                    anchors.right: parent.right
                    spacing: 6

                    PillButton {
                        visible: controller.status === "streaming"
                        text: "Stop"
                        onClicked: controller.stop()
                    }
                    PillButton {
                        visible: controller.answer.length > 0
                        text: "Copy"
                        onClicked: popup.copyAnswer()
                    }
                }
            }
        }
    }
}
