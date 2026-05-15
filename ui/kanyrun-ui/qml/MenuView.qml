import QtQuick
import QtQuick.Controls

FocusScope {
    id: root

    property var actions: []
    property bool submenuMode: false
    property alias currentIndex: actionList.currentIndex
    readonly property int rowHeight: 28
    readonly property int menuWidth: 180

    signal highlightChanged(int index, real itemY)
    signal submenuRequested(int index, real itemY, bool takeFocus)
    signal leafActivated(int index)
    signal cancelRequested()
    signal leftRequested()

    implicitWidth: menuWidth
    implicitHeight: Math.max(0, actions.length * rowHeight) + 8

    function hasSubmenu(index) {
        return index >= 0 && index < actions.length && actions[index].submenu !== undefined && actions[index].submenu !== null
    }

    function isSeparator(index) {
        return index >= 0 && index < actions.length && actions[index].isSeparator === true
    }

    function nextSelectableIndex(fromIndex, step) {
        let index = fromIndex
        while (index >= 0 && index < actions.length) {
            if (!isSeparator(index)) {
                return index
            }
            index += step
        }
        return -1
    }

    function itemYFor(index) {
        const item = actionList.itemAtIndex(index)
        if (!item) {
            return 4
        }
        return item.mapToItem(root, 0, 0).y
    }

    function focusMenu() {
        actionList.forceActiveFocus()
    }

    onActionsChanged: {
        actionList.currentIndex = nextSelectableIndex(0, 1)
    }

    Rectangle {
        anchors.fill: parent
        radius: 6
        color: "#252525"
        border.color: "#4b4b4b"
    }

    ListView {
        id: actionList
        anchors.fill: parent
        anchors.margins: 4
        clip: true
        focus: true
        model: root.actions
        currentIndex: root.nextSelectableIndex(0, 1)

        onCurrentIndexChanged: {
            root.highlightChanged(currentIndex, root.itemYFor(currentIndex))
        }

        Keys.onPressed: function(event) {
            if (event.key === Qt.Key_Down) {
                const nextIndex = root.nextSelectableIndex(currentIndex + 1, 1)
                if (nextIndex !== -1) {
                    currentIndex = nextIndex
                    positionViewAtIndex(currentIndex, ListView.Contain)
                }
                event.accepted = true
            } else if (event.key === Qt.Key_Up) {
                const previousIndex = root.nextSelectableIndex(currentIndex - 1, -1)
                if (previousIndex !== -1) {
                    currentIndex = previousIndex
                    positionViewAtIndex(currentIndex, ListView.Contain)
                }
                event.accepted = true
            } else if (event.key === Qt.Key_Right && root.hasSubmenu(currentIndex)) {
                root.submenuRequested(currentIndex, root.itemYFor(currentIndex), true)
                event.accepted = true
            } else if (event.key === Qt.Key_Left && root.submenuMode) {
                root.leftRequested()
                event.accepted = true
            } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                if (root.isSeparator(currentIndex)) {
                    event.accepted = true
                } else if (root.hasSubmenu(currentIndex)) {
                    root.submenuRequested(currentIndex, root.itemYFor(currentIndex), true)
                    event.accepted = true
                } else if (currentIndex >= 0) {
                    root.leafActivated(currentIndex)
                    event.accepted = true
                }
            } else if (event.key === Qt.Key_Escape) {
                root.cancelRequested()
                event.accepted = true
            }
        }

        delegate: Rectangle {
            id: delegateRoot

            required property int index
            required property var modelData

            width: actionList.width
            height: root.rowHeight
            color: !modelData.isSeparator && ListView.isCurrentItem ? "#3b6ea8" : "transparent"

            MouseArea {
                anchors.fill: parent
                hoverEnabled: !modelData.isSeparator
                enabled: !modelData.isSeparator

                onEntered: {
                    actionList.currentIndex = index
                    if (root.hasSubmenu(index)) {
                        root.submenuRequested(index, root.itemYFor(index), false)
                    }
                }

                onClicked: {
                    if (root.hasSubmenu(index)) {
                        root.submenuRequested(index, root.itemYFor(index), true)
                    } else {
                        root.leafActivated(index)
                    }
                }
            }

            Rectangle {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: 10
                anchors.rightMargin: 10
                height: 1
                color: "#4b4b4b"
                visible: modelData.isSeparator
            }

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.label
                color: "white"
                font.pixelSize: 13
                visible: !modelData.isSeparator
            }

            Text {
                anchors.right: parent.right
                anchors.rightMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                text: root.hasSubmenu(index) ? "▶" : ""
                color: "#d0d0d0"
                font.pixelSize: 11
                visible: !modelData.isSeparator
            }
        }
    }
}
