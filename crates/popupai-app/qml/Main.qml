import QtQuick
import QtQuick.Window
import QtQuick.Effects
import QtQuick.Shapes
import org.kde.layershell 1.0 as LayerShell
import io.github.jeremiznoo.popupai

Window {
    id: popup

    // The application stays resident: dismissing hides the window and a later
    // command shows it again. Development aid: POPUPAI_EXIT_ON_DISMISS=1 quits
    // on dismissal instead.
    readonly property bool oneShot: controller.exitOnDismiss

    readonly property int maxHeight: 600
    property bool wasActive: false
    property bool pickerOpen: false
    property bool settingsOpen: false
    property var allSettings: []
    property bool helpOpen: false
    readonly property bool overlayOpen: pickerOpen || settingsOpen || helpOpen

    // The identity of the application (branding/BRAND.md): the gradient of the logo, one
    // coral spark, and a violet that reads on both light and dark system themes.
    readonly property color brandViolet: "#6B4BFA"
    readonly property color brandBlue: "#3A5BF5"
    readonly property color brandCyan: "#18D2FF"
    readonly property color brandPink: "#B85CFF"
    readonly property color brandCoral: "#FC7C79"
    readonly property color accentText: palette.window.hslLightness > 0.5 ? "#5B3DE0" : "#A78BFA"
    property var allModels: []

    // The popup starts narrow and, as the answer gets longer, widens towards a square so
    // that a long answer is comfortable to read.
    readonly property int answerWidth: Math.min(640, 440 + Math.round(controller.answer.length * 0.25))

    visible: false
    // The window is 14 px larger than the card on every side, which is where the shadow lives.
    readonly property int shadowMargin: 14
    width: (overlayOpen ? 520 : answerWidth) + 2 * shadowMargin
    // The input stays at the bottom edge and the answer grows above it, so the field
    // does not move while an answer streams in.
    height: (overlayOpen ? 400 : Math.min(maxHeight, content.implicitHeight + 24)) + 2 * shadowMargin
    Behavior on width { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
    color: "transparent"
    title: "PopUpAI"

    // Placement: bottom-right corner, above panels, keyboard focus on demand
    // (ADR 0002, spikes S1 to S3).
    LayerShell.Window.scope: "popupai"
    LayerShell.Window.layer: LayerShell.Window.LayerOverlay
    LayerShell.Window.anchors: LayerShell.Window.AnchorBottom | LayerShell.Window.AnchorRight
    LayerShell.Window.margins.right: 4
    LayerShell.Window.margins.bottom: 4
    LayerShell.Window.exclusionZone: 0
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityOnDemand

    SystemPalette { id: palette }
    PopupController { id: controller }

    // Used only to write the answer to the clipboard when the user asks for it.
    TextEdit { id: clipboard; visible: false }

    function rebuildList() {
        const query = searchInput.text.trim().toLowerCase()
        listModel.clear()
        for (const entry of allModels) {
            if (query.length > 0
                    && (entry.name + " " + entry.provider_label).toLowerCase().indexOf(query) < 0)
                continue
            listModel.append(entry)
        }
        // Any model name can be typed: the lists are only what the services and the local
        // server say they have.
        const typed = searchInput.text.trim()
        if (typed.length > 0 && controller.providerId.length > 0) {
            let exact = false
            for (const entry of allModels)
                if (entry.name.toLowerCase() === typed.toLowerCase() && entry.provider === controller.providerId)
                    exact = true
            if (!exact)
                listModel.append({
                    group: "Use this name on the current provider",
                    provider: controller.providerId,
                    provider_label: controller.providerLabel.split(" \u00b7 ")[0],
                    name: typed,
                    favorite: false,
                    current: false,
                    requires_key: false
                })
        }
        list.currentIndex = listModel.count > 0 ? 0 : -1
    }

    function rebuildSettings() {
        settingsModel.clear()
        for (const row of allSettings)
            settingsModel.append(row)
        if (settingsList.currentIndex < 0 || settingsList.currentIndex >= settingsModel.count)
            settingsList.currentIndex = settingsModel.count > 0 ? 0 : -1
    }

    function openHelp() {
        pickerOpen = false
        settingsOpen = false
        helpOpen = true
        helpKeys.forceActiveFocus()
    }

    function closeHelp() {
        helpOpen = false
        focusField()
    }

    // Ctrl+P goes round the views: chat, models, settings, and back to the chat.
    function nextView() {
        if (helpOpen)
            closeHelp()
        else if (settingsOpen)
            closeSettings()
        else if (pickerOpen)
            openSettings()
        else
            openPicker()
    }

    // A line starting with "/" is a command, not a question.
    readonly property var commands: [
        { name: "/help", detail: "Keyboard shortcuts and commands" },
        { name: "/shortcuts", detail: "Same as /help" },
        { name: "/models", detail: "Choose the model (Ctrl+P)" },
        { name: "/settings", detail: "Connections and MCP servers (Ctrl+P twice)" }
    ]

    function matchingCommands() {
        const typed = input.text.trim().toLowerCase()
        return commands.filter(c => c.name.indexOf(typed) === 0)
    }

    function runCommand() {
        const typed = input.text.trim().toLowerCase()
        const found = commands.find(c => c.name === typed) || matchingCommands()[0]
        input.text = ""
        const name = found ? found.name : "/help"
        if (name === "/models")
            openPicker()
        else if (name === "/settings")
            openSettings()
        else
            openHelp()
    }

    function openSettings() {
        pickerOpen = false
        helpOpen = false
        controller.loadSettings()
        settingsOpen = true
        rebuildSettings()
        settingsKeys.forceActiveFocus()
    }

    function closeSettings() {
        settingsOpen = false
        focusField()
    }

    function activateSetting() {
        if (settingsList.currentIndex < 0)
            return
        const row = settingsModel.get(settingsList.currentIndex)
        if (row.kind === "provider") {
            controller.connectProvider(row.id)
            closeSettings()
        } else if (row.kind === "mcp") {
            controller.toggleMcp(row.id)
        }
    }

    function removeSetting() {
        if (settingsList.currentIndex < 0)
            return
        const row = settingsModel.get(settingsList.currentIndex)
        if (row.kind === "provider" && row.removable)
            controller.disconnectProvider(row.id)
    }

    function openPicker() {
        settingsOpen = false
        helpOpen = false
        controller.loadModels()
        searchInput.text = ""
        pickerOpen = true
        rebuildList()
        searchInput.forceActiveFocus()
    }

    function closePicker() {
        pickerOpen = false
        focusField()
    }

    function chooseCurrent() {
        if (list.currentIndex < 0)
            return
        const entry = listModel.get(list.currentIndex)
        controller.chooseModel(entry.provider, entry.name)
        closePicker()
    }

    function focusField() {
        if (controller.needsKey && controller.signsIn)
            signInBox.forceActiveFocus()
        else if (controller.needsKey)
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
        appear.restart()
        focusField()
    }

    function closePopup(reason) {
        controller.dismiss(reason)
        visible = false
        wasActive = false
        pickerOpen = false
        settingsOpen = false
        helpOpen = false
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

    // Only on the user's request: the clipboard is never written on its own.
    function copyText(text) {
        clipboard.text = text
        clipboard.selectAll()
        clipboard.copy()
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
    Shortcut {
        sequence: "Esc"
        onActivated: controller.permissionCommand.length > 0 ? controller.answerPermission(false)
                     : popup.helpOpen ? popup.closeHelp()
                     : popup.settingsOpen ? popup.closeSettings()
                     : popup.pickerOpen ? popup.closePicker() : popup.closePopup("escape")
    }
    Shortcut {
        sequence: "Ctrl+,"
        onActivated: popup.settingsOpen ? popup.closeSettings() : popup.openSettings()
    }
    Shortcut {
        sequence: "Ctrl+P"
        onActivated: popup.nextView()
    }
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
            if (popup.visible && !popup.overlayOpen)
                popup.focusField()
        }

        // The list of models arrives asynchronously after Ctrl+P.
        // A command the model wants to run: the question is put to the user, and the
        // keyboard goes to the confirmation until it is answered.
        function onPermissionCommandChanged() {
            if (controller.permissionCommand.length > 0)
                permissionKeys.forceActiveFocus()
            else if (popup.visible)
                popup.focusField()
        }

        function onSettingsJsonChanged() {
            try {
                popup.allSettings = JSON.parse(controller.settingsJson)
            } catch (e) {
                popup.allSettings = []
            }
            if (popup.settingsOpen)
                popup.rebuildSettings()
        }

        function onModelsJsonChanged() {
            try {
                popup.allModels = JSON.parse(controller.modelsJson)
            } catch (e) {
                popup.allModels = []
            }
            if (popup.pickerOpen)
                popup.rebuildList()
        }
    }

    // The logo, drawn from the same coordinates as branding/logo.svg, so it is sharp at any size.
    component BrandMark: Item {
        property real side: 16
        width: side
        height: side

        Item {
            width: 1024
            height: 1024
            scale: parent.side / 1024
            transformOrigin: Item.TopLeft

            Shape {
                anchors.fill: parent
                preferredRendererType: Shape.CurveRenderer

                ShapePath {
                    strokeWidth: 22
                    strokeColor: popup.brandBlue
                    joinStyle: ShapePath.RoundJoin
                    fillGradient: LinearGradient {
                        x1: 150; y1: 150; x2: 870; y2: 905
                        GradientStop { position: 0.0; color: popup.brandPink }
                        GradientStop { position: 0.38; color: popup.brandViolet }
                        GradientStop { position: 0.64; color: popup.brandBlue }
                        GradientStop { position: 1.0; color: popup.brandCyan }
                    }
                    PathSvg {
                        path: "M340 150 H684 A190 190 0 0 1 874 340 V650 A190 190 0 0 1 684 840 C704 872 732 900 764 910 C694 906 626 882 584 840 H340 A190 190 0 0 1 150 650 V340 A190 190 0 0 1 340 150 Z"
                    }
                }
                ShapePath {
                    strokeWidth: 16
                    strokeColor: "#FCFCFD"
                    fillColor: "#FCFCFD"
                    joinStyle: ShapePath.RoundJoin
                    PathSvg {
                        path: "M611 388 C623 454 668 497 728 509 C668 521 623 564 611 630 C599 564 554 521 494 509 C554 497 599 454 611 388 Z"
                    }
                }
            }
            Rectangle { x: 337; y: 318; width: 54; height: 392; radius: 27; color: "#FCFCFD" }
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

    // Soft shadow under the card.
    Rectangle {
        id: shadowSource
        anchors.fill: panel
        radius: panel.radius
        color: "black"
        visible: false
    }
    MultiEffect {
        anchors.fill: panel
        source: shadowSource
        shadowEnabled: true
        shadowColor: "black"
        shadowOpacity: 0.38
        shadowBlur: 1.0
        shadowVerticalOffset: 6
        opacity: panel.opacity
        scale: panel.scale
    }

    Rectangle {
        id: panel
        anchors.fill: parent
        anchors.margins: popup.shadowMargin
        radius: 18
        color: Qt.alpha(palette.window, 0.97)
        border.color: Qt.alpha(palette.windowText, 0.12)
        border.width: 1
        transformOrigin: Item.BottomRight

        // The card fades and settles in when the popup opens.
        ParallelAnimation {
            id: appear
            NumberAnimation { target: panel; property: "opacity"; from: 0; to: 1; duration: 140 }
            NumberAnimation { target: panel; property: "scale"; from: 0.965; to: 1; duration: 180; easing.type: Easing.OutCubic }
        }

        Column {
            id: content
            visible: !popup.overlayOpen
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

            TextButton {
                visible: controller.canConfigureShortcut
                text: "Set the shortcut\u2026"
                font.pixelSize: 12
                onClicked: controller.configureShortcut()
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

            Row {
                id: thinking
                visible: controller.status === "streaming" && controller.answer.length === 0
                spacing: 5
                height: 14

                Repeater {
                    model: 3
                    Rectangle {
                        required property int index
                        width: 7
                        height: 7
                        radius: 3.5
                        anchors.verticalCenter: parent.verticalCenter
                        color: popup.brandViolet
                        opacity: 0.25

                        SequentialAnimation on opacity {
                            running: thinking.visible
                            loops: Animation.Infinite
                            PauseAnimation { duration: index * 160 }
                            NumberAnimation { from: 0.25; to: 1.0; duration: 320 }
                            NumberAnimation { from: 1.0; to: 0.25; duration: 320 }
                            PauseAnimation { duration: (2 - index) * 160 }
                        }
                    }
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

            // Commands typed after "/": the ones that match, Tab completes, Enter runs.
            Column {
                visible: !controller.needsKey && input.text.trim().indexOf("/") === 0
                width: parent.width
                spacing: 2
                Repeater {
                    model: popup.matchingCommands()
                    Row {
                        required property var modelData
                        spacing: 10
                        Text { text: modelData.name; font.pixelSize: 13; font.bold: true; color: popup.accentText }
                        Text { text: modelData.detail; font.pixelSize: 12; color: Qt.alpha(palette.windowText, 0.6) }
                    }
                }
            }

            // The model wants to run a command that is not a plain read: show all of it and wait.
            Rectangle {
                id: permissionBox
                visible: controller.permissionCommand.length > 0
                width: parent.width
                height: permissionColumn.implicitHeight + 20
                radius: 12
                color: Qt.alpha("#d08770", 0.14)
                border.width: 2
                border.color: Qt.alpha("#d08770", 0.8)

                Item {
                    id: permissionKeys
                    anchors.fill: parent
                    focus: controller.permissionCommand.length > 0
                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            controller.answerPermission(true)
                            event.accepted = true
                        } else if (event.key === Qt.Key_Escape) {
                            controller.answerPermission(false)
                            event.accepted = true
                        }
                    }
                }

                Column {
                    id: permissionColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 10
                    spacing: 6

                    Text {
                        text: "Run this command?"
                        font.bold: true
                        font.pixelSize: 13
                        color: palette.windowText
                    }
                    TextEdit {
                        width: parent.width
                        readOnly: true
                        selectByMouse: true
                        activeFocusOnPress: false
                        wrapMode: TextEdit.WrapAnywhere
                        text: controller.permissionCommand
                        font.family: "monospace"
                        font.pixelSize: 12
                        color: palette.text
                    }
                    Text {
                        width: parent.width
                        wrapMode: Text.Wrap
                        text: "It asks because it " + controller.permissionReason + "."
                        font.pixelSize: 11
                        color: Qt.alpha(palette.windowText, 0.65)
                    }
                    Row {
                        spacing: 16
                        TextButton { text: "Run  (Enter)"; onClicked: controller.answerPermission(true) }
                        TextButton { text: "Don't run  (Esc)"; onClicked: controller.answerPermission(false) }
                    }
                }
            }

            // The one thing that is always there: the field.
            Rectangle {
                visible: controller.permissionCommand.length === 0
                width: parent.width
                height: Math.min(120, Math.max(38, (controller.needsKey && controller.signsIn ? signInColumn.implicitHeight : controller.needsKey ? keyInput.implicitHeight : input.contentHeight) + 18))
                radius: 12
                color: palette.base
                border.width: (input.activeFocus || keyInput.activeFocus || signInBox.activeFocus) ? 2 : 1
                border.color: (input.activeFocus || keyInput.activeFocus || signInBox.activeFocus)
                    ? Qt.alpha(popup.brandViolet, 0.9) : Qt.alpha(palette.windowText, 0.16)

                // Send button: appears once there is something to send.
                Rectangle {
                    id: sendButton
                    visible: !controller.needsKey && input.text.trim().length > 0
                    width: 26
                    height: 26
                    radius: 13
                    anchors.right: parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0.0; color: sendArea.containsMouse ? Qt.lighter(popup.brandViolet, 1.2) : popup.brandViolet }
                        GradientStop { position: 1.0; color: sendArea.containsMouse ? Qt.lighter(popup.brandBlue, 1.2) : popup.brandBlue }
                    }

                    Text {
                        anchors.centerIn: parent
                        text: "\u2191"
                        font.pixelSize: 15
                        font.bold: true
                        color: "#FCFCFD"
                    }
                    MouseArea {
                        id: sendArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: popup.submit()
                    }
                }

                // For a provider used through an account (ChatGPT): Enter starts the
                // sign-in, which is confirmed in the browser.
                Item {
                    id: signInBox
                    visible: controller.needsKey && controller.signsIn
                    anchors.fill: parent
                    anchors.margins: 9

                    Column {
                        id: signInColumn
                        width: parent.width
                        spacing: 5

                        Text {
                            width: parent.width
                            wrapMode: Text.Wrap
                            text: controller.signInCode.length > 0
                                ? "Open the page and enter this code, then confirm:"
                                : "Press Enter to sign in with your ChatGPT account"
                            font.pixelSize: 13
                            color: palette.text
                        }

                        // Selectable, so the code and the link can be copied by hand too.
                        TextEdit {
                            visible: controller.signInCode.length > 0
                            width: parent.width
                            readOnly: true
                            selectByMouse: true
                            activeFocusOnPress: false
                            text: controller.signInCode
                            font.pixelSize: 20
                            font.bold: true
                            font.letterSpacing: 2
                            color: popup.accentText
                        }

                        TextEdit {
                            visible: controller.signInCode.length > 0
                            width: parent.width
                            readOnly: true
                            selectByMouse: true
                            activeFocusOnPress: false
                            wrapMode: TextEdit.WrapAnywhere
                            text: controller.signInUrl
                            font.pixelSize: 11
                            color: Qt.alpha(palette.text, 0.65)
                        }

                        Row {
                            visible: controller.signInCode.length > 0
                            spacing: 14
                            TextButton { text: "Copy code"; onClicked: popup.copyText(controller.signInCode) }
                            TextButton { text: "Copy link"; onClicked: popup.copyText(controller.signInUrl) }
                            TextButton {
                                text: "Open link"
                                onClicked: {
                                    if (controller.signInUrl.indexOf("https://") === 0)
                                        Qt.openUrlExternally(controller.signInUrl)
                                }
                            }
                        }
                    }

                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            controller.signIn()
                            event.accepted = true
                        }
                    }
                }

                TextEdit {
                    id: input
                    visible: !controller.needsKey
                    anchors.fill: parent
                    anchors.margins: 10
                    anchors.rightMargin: 40
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
                        const isEnter = (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                                && !(event.modifiers & Qt.ShiftModifier)
                        if (isEnter && input.text.trim().indexOf("/") === 0) {
                            popup.runCommand()
                            event.accepted = true
                        } else if (event.key === Qt.Key_Tab && input.text.trim().indexOf("/") === 0
                                   && popup.matchingCommands().length > 0) {
                            input.text = popup.matchingCommands()[0].name
                            input.cursorPosition = input.text.length
                            event.accepted = true
                        } else if (isEnter) {
                            popup.submit()
                            event.accepted = true
                        }
                    }
                }

                // Shown instead of the question field when the provider needs an API key
                // that is not stored yet. Masked, and handed straight to the secret store.
                TextInput {
                    id: keyInput
                    visible: controller.needsKey && !controller.signsIn
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

                BrandMark { id: footMark; side: 12; anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter }

                TextButton {
                    anchors.left: footMark.right
                    anchors.leftMargin: 6
                    anchors.right: actions.left
                    anchors.rightMargin: 8
                    elide: Text.ElideRight
                    text: controller.needsKey && !controller.signsIn
                        ? controller.providerLabel + " · key goes to the system wallet"
                        : controller.providerLabel
                    onClicked: popup.openPicker()
                }

                Row {
                    id: actions
                    anchors.right: parent.right
                    spacing: 12

                    TextButton {
                        text: "\u2699 Settings"
                        onClicked: popup.openSettings()
                    }

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

        // Help (/help or /shortcuts): every key, in one place.
        Item {
            id: helpView
            visible: popup.helpOpen
            anchors.fill: parent
            anchors.margins: 14

            Item {
                id: helpHeader
                width: parent.width
                height: 20
                BrandMark { id: helpMark; side: 18; anchors.verticalCenter: parent.verticalCenter }
                Text {
                    anchors.left: helpMark.right
                    anchors.leftMargin: 8
                    text: "Shortcuts"
                    font.bold: true
                    font.pixelSize: 13
                    color: palette.windowText
                }
                Text {
                    anchors.right: parent.right
                    text: "esc"
                    font.pixelSize: 12
                    color: Qt.alpha(palette.windowText, 0.5)
                }
            }

            Item { id: helpKeys; anchors.fill: parent; focus: popup.helpOpen }

            Flickable {
                anchors {
                    top: helpHeader.bottom
                    topMargin: 8
                    bottom: parent.bottom
                    left: parent.left
                    right: parent.right
                }
                clip: true
                contentHeight: helpRows.implicitHeight
                interactive: contentHeight > height

                Column {
                    id: helpRows
                    width: parent.width
                    spacing: 3

                    component Heading: Text {
                        width: helpRows.width
                        topPadding: 8
                        font.bold: true
                        font.pixelSize: 12
                        color: popup.accentText
                    }
                    component Line: Row {
                        property string keys
                        property string what
                        spacing: 10
                        Text { width: 120; text: parent.keys; font.pixelSize: 12; font.bold: true; color: palette.windowText }
                        Text { width: helpRows.width - 130; text: parent.what; font.pixelSize: 12; wrapMode: Text.Wrap; color: Qt.alpha(palette.windowText, 0.75) }
                    }

                    Heading { text: "Chat" }
                    Line { keys: "Enter"; what: "Send" }
                    Line { keys: "Shift+Enter"; what: "New line" }
                    Line { keys: "/"; what: "Commands: /help, /shortcuts, /models, /settings (Tab completes)" }
                    Line { keys: "Ctrl+."; what: "Stop the answer" }
                    Line { keys: "Ctrl+R"; what: "Bring back an answer that finished in the background" }
                    Line { keys: "Ctrl+Shift+C"; what: "Copy the answer" }
                    Line { keys: "Esc"; what: "Close the popup (or the open page)" }
                    Heading { text: "Views" }
                    Line { keys: "Ctrl+P"; what: "Next view: chat, models, settings, then back to the chat" }
                    Line { keys: "Ctrl+,"; what: "Settings" }
                    Heading { text: "Models" }
                    Line { keys: "Up / Down, Enter"; what: "Move and choose; type to search or to use any model name" }
                    Line { keys: "Ctrl+F"; what: "Favourite" }
                    Line { keys: "Ctrl+A"; what: "Connect a provider" }
                    Heading { text: "Settings" }
                    Line { keys: "Enter"; what: "Connect a provider, or turn an MCP server on or off" }
                    Line { keys: "Del"; what: "Disconnect a provider" }
                    Heading { text: "A command the model asks to run" }
                    Line { keys: "Enter / Esc"; what: "Run it / do not run it" }
                }
            }
        }

        // Settings (Ctrl+, or the gear): connections to providers and a few general options.
        Item {
            id: settingsView
            visible: popup.settingsOpen
            anchors.fill: parent
            anchors.margins: 14

            Item {
                id: settingsHeader
                width: parent.width
                height: 20
                BrandMark { id: settingsMark; side: 18; anchors.verticalCenter: parent.verticalCenter }
                Text {
                    anchors.left: settingsMark.right
                    anchors.leftMargin: 8
                    text: "Settings"
                    font.bold: true
                    font.pixelSize: 13
                    color: palette.windowText
                }
                Text {
                    anchors.right: parent.right
                    text: "esc"
                    font.pixelSize: 12
                    color: Qt.alpha(palette.windowText, 0.5)
                }
            }

            // Takes the keyboard while the page is open.
            Item {
                id: settingsKeys
                anchors.fill: parent
                focus: popup.settingsOpen
                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Down) {
                        settingsList.currentIndex = Math.min(settingsList.count - 1, settingsList.currentIndex + 1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Up) {
                        settingsList.currentIndex = Math.max(0, settingsList.currentIndex - 1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        popup.activateSetting()
                        event.accepted = true
                    } else if (event.key === Qt.Key_Delete || event.key === Qt.Key_Backspace) {
                        popup.removeSetting()
                        event.accepted = true
                    }
                }
            }

            ListModel { id: settingsModel }

            ListView {
                id: settingsList
                anchors {
                    top: settingsHeader.bottom
                    topMargin: 8
                    bottom: settingsFooter.top
                    bottomMargin: 8
                    left: parent.left
                    right: parent.right
                }
                clip: true
                model: settingsModel
                highlightMoveDuration: 0
                boundsBehavior: Flickable.StopAtBounds

                section.property: "group"
                section.criteria: ViewSection.FullString
                section.delegate: Text {
                    width: settingsList.width
                    height: 26
                    verticalAlignment: Text.AlignBottom
                    text: section
                    font.bold: true
                    font.pixelSize: 12
                    color: popup.accentText
                }

                delegate: Rectangle {
                    width: settingsList.width
                    height: 38
                    radius: 8
                    color: ListView.isCurrentItem ? Qt.alpha(popup.brandViolet, 0.28) : "transparent"

                    Column {
                        anchors.left: parent.left
                        anchors.leftMargin: 10
                        anchors.right: parent.right
                        anchors.rightMargin: 10
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 1
                        Text {
                            width: parent.width
                            elide: Text.ElideRight
                            text: model.title
                            font.pixelSize: 13
                            color: palette.windowText
                        }
                        Text {
                            width: parent.width
                            elide: Text.ElideRight
                            text: model.detail
                            font.pixelSize: 11
                            color: Qt.alpha(palette.windowText, 0.55)
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            settingsList.currentIndex = index
                            popup.activateSetting()
                        }
                    }
                }
            }

            Text {
                id: settingsFooter
                anchors.bottom: parent.bottom
                text: "enter choose \u00b7 del disconnect \u00b7 esc close"
                font.pixelSize: 11
                color: Qt.alpha(palette.windowText, 0.5)
            }
        }

        // Model picker (Ctrl+P): search, recent and favourite models, then every provider.
        Item {
            id: picker
            visible: popup.pickerOpen
            anchors.fill: parent
            anchors.margins: 14

            Item {
                id: pickerHeader
                width: parent.width
                height: 20
                BrandMark { id: pickerMark; side: 18; anchors.verticalCenter: parent.verticalCenter }
                Text {
                    anchors.left: pickerMark.right
                    anchors.leftMargin: 8
                    text: "Select model"
                    font.bold: true
                    font.pixelSize: 13
                    color: palette.windowText
                }
                Text {
                    anchors.right: parent.right
                    text: "esc"
                    font.pixelSize: 12
                    color: Qt.alpha(palette.windowText, 0.5)
                }
            }

            TextInput {
                id: searchInput
                anchors.top: pickerHeader.bottom
                anchors.topMargin: 10
                width: parent.width
                height: 24
                color: palette.text
                font.pixelSize: 14
                selectByMouse: true
                onTextChanged: popup.rebuildList()

                Text {
                    visible: searchInput.text.length === 0
                    text: "Search"
                    font: searchInput.font
                    color: Qt.alpha(palette.text, 0.4)
                }

                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Down) {
                        list.currentIndex = Math.min(list.count - 1, list.currentIndex + 1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Up) {
                        list.currentIndex = Math.max(0, list.currentIndex - 1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        popup.chooseCurrent()
                        event.accepted = true
                    } else if ((event.modifiers & Qt.ControlModifier) && event.key === Qt.Key_A) {
                        popup.openSettings()
                        event.accepted = true
                    } else if ((event.modifiers & Qt.ControlModifier) && event.key === Qt.Key_F
                               && list.currentIndex >= 0) {
                        const entry = listModel.get(list.currentIndex)
                        controller.toggleFavorite(entry.provider, entry.name)
                        event.accepted = true
                    }
                }
            }

            ListModel { id: listModel }

            ListView {
                id: list
                anchors {
                    top: searchInput.bottom
                    topMargin: 10
                    bottom: pickerFooter.top
                    bottomMargin: 8
                    left: parent.left
                    right: parent.right
                }
                clip: true
                model: listModel
                currentIndex: 0
                highlightMoveDuration: 0
                boundsBehavior: Flickable.StopAtBounds

                section.property: "group"
                section.criteria: ViewSection.FullString
                section.delegate: Text {
                    width: list.width
                    height: 24
                    verticalAlignment: Text.AlignBottom
                    text: section
                    font.bold: true
                    font.pixelSize: 12
                    color: popup.accentText
                }

                delegate: Rectangle {
                    width: list.width
                    height: 26
                    radius: 5
                    color: ListView.isCurrentItem ? Qt.alpha(popup.brandViolet, 0.28) : "transparent"

                    Text {
                        anchors.left: parent.left
                        anchors.leftMargin: 8
                        anchors.right: hint.left
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        elide: Text.ElideRight
                        text: (current ? "\u25cf " : "") + name
                        font.pixelSize: 13
                        font.bold: current
                        color: palette.windowText
                    }
                    Text {
                        id: hint
                        anchors.right: parent.right
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        text: (favorite ? "\u2605 " : "")
                              + provider_label
                              + (requires_key ? " \u00b7 key" : "")
                        font.pixelSize: 11
                        color: Qt.alpha(palette.windowText, 0.55)
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            list.currentIndex = index
                            popup.chooseCurrent()
                        }
                    }
                }
            }

            Text {
                id: pickerFooter
                anchors.bottom: parent.bottom
                text: "enter select \u00b7 ctrl+f favorite \u00b7 ctrl+a connect \u00b7 esc close"
                font.pixelSize: 11
                color: Qt.alpha(palette.windowText, 0.5)
            }
        }
    }
}
