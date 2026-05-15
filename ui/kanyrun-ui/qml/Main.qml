import QtQuick
import QtQuick.Controls

Window {
    id: window
    width: mainMenu.implicitWidth
    height: mainMenu.implicitHeight
    visible: false
    color: "transparent"
    flags: Qt.Popup | Qt.Tool | Qt.FramelessWindowHint | Qt.NoDropShadowWindowHint | Qt.WindowDoesNotAcceptFocus

    property var submenuActions: []
    property int submenuParentIndex: -1

    function submenuPosition(itemY) {
        const activeScreen = window.screen ? window.screen : Screen
        const popupWidth = submenuMenu.implicitWidth
        const popupHeight = submenuMenu.implicitHeight
        const screenLeft = activeScreen.virtualX + 8
        const screenTop = activeScreen.virtualY + 8
        const screenRight = activeScreen.virtualX + activeScreen.width - popupWidth - 8
        const screenBottom = activeScreen.virtualY + activeScreen.height - popupHeight - 8

        const rowGlobalTopLeft = mainMenu.mapToGlobal(0, itemY + 4)
        const rowGlobalTopRight = mainMenu.mapToGlobal(mainMenu.width - 1, itemY + 4)
        const rowGlobalBottomLeft = mainMenu.mapToGlobal(0, itemY + mainMenu.rowHeight + 4)
        const preferredRightX = rowGlobalTopRight.x
        const preferredLeftX = rowGlobalTopLeft.x - popupWidth + 1
        const fitsRight = preferredRightX <= screenRight
        const fitsLeft = preferredLeftX >= screenLeft

        let globalX = fitsRight || !fitsLeft ? preferredRightX : preferredLeftX
        globalX = Math.max(screenLeft, Math.min(globalX, screenRight))

        let globalY = rowGlobalTopLeft.y
        if (globalY + popupHeight > screenBottom) {
            globalY = rowGlobalBottomLeft.y - popupHeight
        }
        globalY = Math.max(screenTop, Math.min(globalY, screenBottom))

        return Qt.point(globalX - window.x, globalY - window.y)
    }

    function closeSubmenu() {
        submenuPopup.close()
        submenuActions = []
        submenuParentIndex = -1
    }

    function openSubmenu(index, itemY, takeFocus) {
        if (!mainMenu.hasSubmenu(index)) {
            closeSubmenu()
            return
        }

        submenuParentIndex = index
        submenuActions = mainMenu.actions[index].submenu
        submenuMenu.currentIndex = submenuMenu.nextSelectableIndex(0, 1)
        const position = submenuPosition(itemY)
        submenuPopup.x = position.x
        submenuPopup.y = position.y
        submenuPopup.open()

        if (takeFocus) {
            submenuMenu.focusMenu()
        }
    }

    function cancelMenu() {
        menuController.cancel()
    }

    Component.onCompleted: {
        Qt.callLater(function() {
            mainMenu.currentIndex = menuController.defaultIndex >= 0 && menuController.defaultIndex < menuController.actions.length ? menuController.defaultIndex : 0
            mainMenu.focusMenu()
        })
    }

    MenuView {
        id: mainMenu
        anchors.fill: parent
        actions: menuController.actions
        submenuMode: false

        onHighlightChanged: function(index, itemY) {
            if (!submenuPopup.visible) {
                return
            }

            if (mainMenu.hasSubmenu(index)) {
                openSubmenu(index, itemY, false)
            } else {
                closeSubmenu()
            }
        }

        onSubmenuRequested: function(index, itemY, takeFocus) {
            openSubmenu(index, itemY, takeFocus)
        }

        onLeafActivated: function(index) {
            menuController.activatePath([index])
        }

        onCancelRequested: cancelMenu()
    }

    Popup {
        id: submenuPopup
        padding: 0
        modal: false
        focus: true
        popupType: Popup.Window
        closePolicy: Popup.NoAutoClose
        background: Item {}

        onOpened: submenuMenu.focusMenu()

        contentItem: MenuView {
            id: submenuMenu
            actions: window.submenuActions
            submenuMode: true

            onLeafActivated: function(index) {
                menuController.activatePath([submenuParentIndex, index])
            }

            onCancelRequested: cancelMenu()

            onLeftRequested: {
                closeSubmenu()
                mainMenu.focusMenu()
            }
        }
    }
}
