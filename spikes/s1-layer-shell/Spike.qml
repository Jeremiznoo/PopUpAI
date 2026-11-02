import QtQuick

Rectangle {
    id: root
    radius: 14
    color: "#1e2127"
    border.color: "#3b4252"
    border.width: 1

    property int keyCount: 0

    Column {
        anchors.fill: parent
        anchors.margins: 18
        spacing: 8

        Text {
            text: "PopUpAI spike S1"
            color: "#eceff4"
            font.pixelSize: 18
            font.bold: true
        }
        Text {
            width: parent.width
            text: "Type a few letters, then click on another window.\nEsc closes this window."
            color: "#d8dee9"
            font.pixelSize: 13
            wrapMode: Text.Wrap
        }
        Rectangle {
            width: parent.width
            height: 36
            radius: 8
            color: "#2e3440"
            border.color: input.activeFocus ? "#88c0d0" : "#434c5e"

            TextInput {
                id: input
                anchors.fill: parent
                anchors.margins: 8
                color: "#eceff4"
                font.pixelSize: 14
                focus: true
                selectByMouse: true

                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Escape) {
                        spike.escape()
                    } else {
                        root.keyCount++
                        spike.keyPressed()
                    }
                }
            }
        }
        Text {
            text: "keys received: " + root.keyCount + "    input has focus: " + input.activeFocus
            color: "#a3be8c"
            font.pixelSize: 12
        }
    }
}
