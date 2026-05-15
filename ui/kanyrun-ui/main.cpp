#include <LayerShellQt/window.h>
#include <QApplication>
#include <QCoreApplication>
#include <QCursor>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusReply>
#include <QDir>
#include <QEnterEvent>
#include <QEventLoop>
#include <QFile>
#include <QFrame>
#include <QHBoxLayout>
#include <QIcon>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeyEvent>
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QProcess>
#include <QScreen>
#include <QTemporaryFile>
#include <QTextStream>
#include <QTimer>
#include <QUuid>
#include <QVBoxLayout>
#include <QWidget>
#include <QWindow>
#include <algorithm>
#include <iostream>
#include <limits>
#include <memory>
#include <optional>
#include <utility>
#include <vector>

static std::string readStdin()
{
    return {std::istreambuf_iterator<char>(std::cin), std::istreambuf_iterator<char>()};
}

static bool readFramedStdin(std::string *payload)
{
    if (payload == nullptr) {
        return false;
    }

    std::string lengthLine;
    if (!std::getline(std::cin, lengthLine)) {
        return false;
    }

    if (lengthLine.empty()) {
        payload->clear();
        return true;
    }

    std::size_t length = 0;
    try {
        length = static_cast<std::size_t>(std::stoull(lengthLine));
    } catch (...) {
        return false;
    }

    payload->assign(length, '\0');
    std::cin.read(payload->data(), static_cast<std::streamsize>(length));
    return std::cin.good() || std::cin.gcount() == static_cast<std::streamsize>(length);
}

struct MenuActionModel {
    QString id;
    QString label;
    QString iconName;
    bool isDefault = false;
    bool isSeparator = false;
    std::vector<MenuActionModel> submenu;
};

static MenuActionModel parseAction(const QJsonObject &object)
{
    MenuActionModel action;
    action.id = object.value("id").toString();
    action.label = object.value("label").toString();
    action.iconName = object.value("icon").toString();
    action.isDefault = object.value("is_default").toBool();
    action.isSeparator = object.value("is_separator").toBool();

    const auto submenu = object.value("submenu");
    if (submenu.isArray()) {
        const auto rawSubmenu = submenu.toArray();
        action.submenu.reserve(rawSubmenu.size());
        for (const auto item : rawSubmenu) {
            action.submenu.push_back(parseAction(item.toObject()));
        }
    }

    return action;
}

static std::vector<MenuActionModel> parseActions(const QJsonArray &rawActions)
{
    std::vector<MenuActionModel> actions;
    actions.reserve(rawActions.size());
    for (const auto item : rawActions) {
        actions.push_back(parseAction(item.toObject()));
    }
    return actions;
}

struct MenuPayloadModel {
    QString title;
    bool showIcons = true;
    std::vector<MenuActionModel> actions;
};

static MenuPayloadModel parseMenuPayload(const QJsonObject &object)
{
    MenuPayloadModel payload;
    payload.title = object.value("title").toString();
    payload.showIcons = object.contains("show_icons") ? object.value("show_icons").toBool() : true;
    payload.actions = parseActions(object.value("actions").toArray());
    return payload;
}

class CursorReplyHandler final : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.kanyrun.Cursor")

public:
    std::optional<QPoint> cursor;
    QEventLoop *loop = nullptr;

public slots:
    void ReportCursor(int x, int y)
    {
        cursor = QPoint(x, y);
        if (loop != nullptr) {
            loop->quit();
        }
    }
};

struct CursorAnchor {
    QPoint position;
    QScreen *screen = nullptr;
    QString source;
};

static std::optional<QPoint> readCursorPositionFromKWin()
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        return std::nullopt;
    }

    const QString requestId = QUuid::createUuid().toString(QUuid::WithoutBraces).remove('-');
    const QString serviceName = QStringLiteral("org.kanyrun.Cursor.p%1.%2")
                                    .arg(QCoreApplication::applicationPid())
                                    .arg(requestId);
    const QString pluginName = QStringLiteral("kanyrun-cursor-%1").arg(requestId);
    const QString objectPath = QStringLiteral("/org/kanyrun/Cursor");

    CursorReplyHandler handler;
    if (!bus.registerService(serviceName)) {
        return std::nullopt;
    }
    if (!bus.registerObject(objectPath, &handler, QDBusConnection::ExportAllSlots)) {
        bus.unregisterService(serviceName);
        return std::nullopt;
    }

    QTemporaryFile scriptFile(QDir::tempPath() + QStringLiteral("/kanyrun-cursor-XXXXXX.js"));
    if (!scriptFile.open()) {
        bus.unregisterObject(objectPath);
        bus.unregisterService(serviceName);
        return std::nullopt;
    }

    const QString script = QStringLiteral(R"JS((function () {
    const pos = workspace.cursorPos;
    callDBus("%1", "/org/kanyrun/Cursor", "org.kanyrun.Cursor", "ReportCursor", pos.x, pos.y);
})();
)JS")
                               .arg(serviceName);
    if (scriptFile.write(script.toUtf8()) < 0) {
        bus.unregisterObject(objectPath);
        bus.unregisterService(serviceName);
        return std::nullopt;
    }
    scriptFile.flush();
    scriptFile.close();

    QDBusInterface scripting(QStringLiteral("org.kde.KWin"),
                             QStringLiteral("/Scripting"),
                             QStringLiteral("org.kde.kwin.Scripting"),
                             bus);
    if (!scripting.isValid()) {
        bus.unregisterObject(objectPath);
        bus.unregisterService(serviceName);
        return std::nullopt;
    }

    const auto unloadScript = [&]() {
        scripting.call(QStringLiteral("unloadScript"), pluginName);
        bus.unregisterObject(objectPath);
        bus.unregisterService(serviceName);
    };

    const QDBusReply<int> loadReply = scripting.call(QStringLiteral("loadScript"), scriptFile.fileName(), pluginName);
    if (!loadReply.isValid()) {
        unloadScript();
        return std::nullopt;
    }

    const QDBusMessage startReply = scripting.call(QStringLiteral("start"));
    if (startReply.type() == QDBusMessage::ErrorMessage) {
        unloadScript();
        return std::nullopt;
    }

    QEventLoop loop;
    QTimer timer;
    timer.setSingleShot(true);
    QObject::connect(&timer, &QTimer::timeout, &loop, &QEventLoop::quit);
    handler.loop = &loop;
    timer.start(500);
    loop.exec();
    handler.loop = nullptr;

    const std::optional<QPoint> cursor = handler.cursor;
    unloadScript();
    return cursor;
}

static bool isWaylandSession()
{
    static const bool wayland = !qEnvironmentVariableIsEmpty("WAYLAND_DISPLAY");
    return wayland;
}

static std::optional<QPoint> readCursorPositionFromQt()
{
    const QPoint cursor = QCursor::pos();
    return cursor.isNull() ? std::nullopt : std::optional<QPoint>(cursor);
}

static std::optional<QPoint> readCursorPositionOnce(QString *source)
{
    if (isWaylandSession()) {
        if (const auto kwinCursor = readCursorPositionFromKWin()) {
            if (source != nullptr) {
                *source = QStringLiteral("kwin");
            }
            return kwinCursor;
        }
        return std::nullopt;
    }

    if (const auto qtCursor = readCursorPositionFromQt()) {
        if (source != nullptr) {
            *source = QStringLiteral("qt");
        }
        return qtCursor;
    }

    return std::nullopt;
}

static QScreen *bestScreenForCursor(const QPoint &point)
{
    if (QScreen *screen = QApplication::screenAt(point)) {
        return screen;
    }

    const auto screens = QApplication::screens();
    if (screens.isEmpty()) {
        return nullptr;
    }

    QScreen *best = screens.front();
    qint64 bestDistance = std::numeric_limits<qint64>::max();
    for (QScreen *screen : screens) {
        const QRect available = screen->availableGeometry();
        const int dx = point.x() < available.left() ? available.left() - point.x()
            : point.x() > available.right() ? point.x() - available.right()
            : 0;
        const int dy = point.y() < available.top() ? available.top() - point.y()
            : point.y() > available.bottom() ? point.y() - available.bottom()
            : 0;
        const qint64 distance = static_cast<qint64>(dx) * dx + static_cast<qint64>(dy) * dy;
        if (distance < bestDistance) {
            bestDistance = distance;
            best = screen;
        }
    }

    return best;
}

static std::optional<CursorAnchor> resolveCursorAnchorFromEnvironment()
{
    bool okX = false;
    bool okY = false;
    const int x = qEnvironmentVariableIntValue("KANYRUN_CURSOR_X", &okX);
    const int y = qEnvironmentVariableIntValue("KANYRUN_CURSOR_Y", &okY);
    if (!okX || !okY) {
        return std::nullopt;
    }

    const QPoint position(x, y);
    QScreen *screen = bestScreenForCursor(position);
    if (screen == nullptr) {
        return std::nullopt;
    }

    QString source = QStringLiteral("env");
    const QString envSource = qEnvironmentVariable("KANYRUN_CURSOR_SOURCE");
    if (!envSource.isEmpty()) {
        source = QStringLiteral("env-%1").arg(envSource);
    }

    return CursorAnchor{position, screen, source};
}

static std::optional<CursorAnchor> fallbackCursorAnchor()
{
    QScreen *screen = QApplication::primaryScreen();
    if (screen == nullptr) {
        const auto screens = QApplication::screens();
        if (screens.isEmpty()) {
            return std::nullopt;
        }
        screen = screens.front();
    }

    const QRect available = screen->availableGeometry();
    return CursorAnchor{available.center(), screen, QStringLiteral("fallback-primary-screen-center")};
}

static std::optional<CursorAnchor> resolveCursorAnchor()
{
    QString source;
    const std::optional<QPoint> position = readCursorPositionOnce(&source);
    if (!position) {
        return fallbackCursorAnchor();
    }

    QScreen *screen = bestScreenForCursor(*position);
    if (screen == nullptr) {
        return fallbackCursorAnchor();
    }

    return CursorAnchor{*position, screen, source};
}

static int printCursorAnchor(bool allowFallback)
{
    std::optional<CursorAnchor> anchor;
    if (allowFallback) {
        anchor = resolveCursorAnchor();
    } else {
        QString source;
        const std::optional<QPoint> position = readCursorPositionOnce(&source);
        if (position) {
            if (QScreen *screen = bestScreenForCursor(*position)) {
                anchor = CursorAnchor{*position, screen, source};
            }
        }
    }

    if (!anchor || anchor->screen == nullptr) {
        return 1;
    }

    QTextStream out(stdout);
    out << "X=" << anchor->position.x() << Qt::endl;
    out << "Y=" << anchor->position.y() << Qt::endl;
    out << "SOURCE=" << anchor->source << Qt::endl;
    out.flush();
    return 0;
}

static void appendPlacementLog(const QString &line)
{
    QFile file(QStringLiteral("/tmp/kanyrun-ui-placement.log"));
    if (!file.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) {
        return;
    }
    QTextStream stream(&file);
    stream << line << Qt::endl;
}

static QScreen *screenForPoint(const QPoint &point)
{
    if (QScreen *screen = QApplication::screenAt(point)) {
        return screen;
    }
    return QApplication::primaryScreen();
}

static QPoint clampMenuPosition(const QPoint &preferredPosition, const QSize &menuSize, const QRect &available)
{
    const QSize size = menuSize.expandedTo(QSize(1, 1));
    const int maxX = std::max(available.left(), available.right() - size.width() + 1);
    const int maxY = std::max(available.top(), available.bottom() - size.height() + 1);

    return {
        std::clamp(preferredPosition.x(), available.left(), maxX),
        std::clamp(preferredPosition.y(), available.top(), maxY),
    };
}

class MenuItemWidget final : public QWidget
{
public:
    explicit MenuItemWidget(const MenuActionModel *action, bool showIcons, QWidget *parent = nullptr)
        : QWidget(parent)
        , m_action(action)
        , m_showIcons(showIcons)
        , m_icon(QIcon::fromTheme(action->iconName))
    {
        setMouseTracking(true);
        setAttribute(Qt::WA_Hover);
        setCursor(Qt::PointingHandCursor);
        setFixedHeight(24);
    }

    [[nodiscard]] const MenuActionModel *action() const
    {
        return m_action;
    }

    [[nodiscard]] bool isHighlighted() const
    {
        return m_highlighted;
    }

    void setHighlighted(bool highlighted)
    {
        if (m_highlighted == highlighted) {
            return;
        }
        m_highlighted = highlighted;
        update();
    }

    std::function<void(MenuItemWidget *)> onHover;
    std::function<void(MenuItemWidget *)> onActivate;

protected:
    void enterEvent(QEnterEvent *event) override
    {
        QWidget::enterEvent(event);
        if (onHover) {
            onHover(this);
        }
    }

    void mouseReleaseEvent(QMouseEvent *event) override
    {
        QWidget::mouseReleaseEvent(event);
        if (event->button() == Qt::LeftButton && rect().contains(event->position().toPoint()) && onActivate) {
            onActivate(this);
        }
    }

    void paintEvent(QPaintEvent *event) override
    {
        QWidget::paintEvent(event);

        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, false);

        const QRect contentRect = rect();
        if (m_highlighted) {
            painter.fillRect(contentRect, QColor(61, 174, 233));
        }

        const int leftPadding = 8;
        const int rightPadding = 6;
        const int iconSize = 16;
        const int arrowWidth = m_action->submenu.empty() ? 0 : 14;
        const int iconAreaWidth = m_showIcons ? 18 : 0;

        if (m_showIcons) {
            const QRect iconRect(leftPadding, (height() - iconSize) / 2, iconSize, iconSize);
            if (!m_icon.isNull()) {
                m_icon.paint(&painter, iconRect);
            }
        }

        painter.setPen(m_highlighted ? Qt::white : QColor(239, 240, 241));
        const QRect textRect(leftPadding + iconAreaWidth,
                             0,
                             width() - leftPadding - rightPadding - iconAreaWidth - arrowWidth,
                             height());
        const QString text = fontMetrics().elidedText(m_action->label, Qt::ElideRight, textRect.width());
        painter.drawText(textRect, Qt::AlignVCenter | Qt::AlignLeft, text);

        if (!m_action->submenu.empty()) {
            const QRect arrowRect(width() - rightPadding - 12, 0, 12, height());
            painter.drawText(arrowRect, Qt::AlignCenter, QStringLiteral("›"));
        }
    }

private:
    const MenuActionModel *m_action;
    bool m_showIcons;
    QIcon m_icon;
    bool m_highlighted = false;
};

class MenuSeparatorWidget final : public QWidget
{
public:
    explicit MenuSeparatorWidget(QWidget *parent = nullptr)
        : QWidget(parent)
    {
        setFixedHeight(7);
    }

protected:
    void paintEvent(QPaintEvent *event) override
    {
        QWidget::paintEvent(event);
        QPainter painter(this);
        painter.setPen(QColor(88, 92, 100));
        const int y = height() / 2;
        painter.drawLine(8, y, width() - 8, y);
    }
};

class MenuPanel final : public QFrame
{
public:
    MenuPanel(const std::vector<MenuActionModel> *actions, bool showIcons, int level, QWidget *parent = nullptr)
        : QFrame(parent)
        , m_actions(actions)
        , m_showIcons(showIcons)
        , m_level(level)
    {
        setFrameStyle(QFrame::NoFrame);
        setObjectName(QStringLiteral("menu-panel"));
        setAttribute(Qt::WA_StyledBackground, true);
        setStyleSheet(QStringLiteral(
            "#menu-panel {"
            "background-color: rgba(36, 38, 41, 245);"
            "border: 1px solid rgba(110, 114, 122, 230);"
            "border-radius: 6px;"
            "}"
        ));

        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(2, 2, 2, 2);
        layout->setSpacing(0);

        int minimumWidth = 180;
        for (const auto &action : *m_actions) {
            if (action.isSeparator) {
                auto *separator = new MenuSeparatorWidget(this);
                layout->addWidget(separator);
                continue;
            }

            auto *item = new MenuItemWidget(&action, m_showIcons, this);
            item->onHover = [this](MenuItemWidget *hovered) {
                setHighlightedItem(hovered);
                if (onItemHovered) {
                    onItemHovered(this, hovered);
                }
            };
            item->onActivate = [this](MenuItemWidget *activated) {
                setHighlightedItem(activated);
                if (onItemActivated) {
                    onItemActivated(this, activated);
                }
            };
            layout->addWidget(item);
            m_items.push_back(item);
            minimumWidth = std::max(minimumWidth, fontMetrics().horizontalAdvance(action.label) + (m_showIcons ? 64 : 46));
        }

        setMinimumWidth(minimumWidth);
        adjustSize();
    }

    [[nodiscard]] int level() const
    {
        return m_level;
    }

    [[nodiscard]] MenuItemWidget *defaultItem() const
    {
        for (auto *item : m_items) {
            if (item->action()->isDefault) {
                return item;
            }
        }
        return m_items.empty() ? nullptr : m_items.front();
    }

    [[nodiscard]] MenuItemWidget *highlightedItem() const
    {
        return m_highlightedItem;
    }

    [[nodiscard]] MenuItemWidget *firstItem() const
    {
        return m_items.empty() ? nullptr : m_items.front();
    }

    [[nodiscard]] int firstItemTop() const
    {
        if (MenuItemWidget *item = firstItem()) {
            return item->geometry().top();
        }
        return contentsMargins().top();
    }

    [[nodiscard]] MenuItemWidget *lastItem() const
    {
        return m_items.empty() ? nullptr : m_items.back();
    }

    [[nodiscard]] MenuItemWidget *nextItem(MenuItemWidget *item, int direction) const
    {
        if (m_items.empty()) {
            return nullptr;
        }

        auto it = std::find(m_items.begin(), m_items.end(), item);
        int index = it == m_items.end() ? 0 : static_cast<int>(std::distance(m_items.begin(), it));
        index = std::clamp(index + direction, 0, static_cast<int>(m_items.size()) - 1);
        return m_items[static_cast<std::size_t>(index)];
    }

    void setHighlightedItem(MenuItemWidget *item)
    {
        if (m_highlightedItem == item) {
            return;
        }

        if (m_highlightedItem != nullptr) {
            m_highlightedItem->setHighlighted(false);
        }
        m_highlightedItem = item;
        if (m_highlightedItem != nullptr) {
            m_highlightedItem->setHighlighted(true);
        }
    }

    std::function<void(MenuPanel *, MenuItemWidget *)> onItemHovered;
    std::function<void(MenuPanel *, MenuItemWidget *)> onItemActivated;

private:
    const std::vector<MenuActionModel> *m_actions;
    bool m_showIcons;
    int m_level;
    std::vector<MenuItemWidget *> m_items;
    MenuItemWidget *m_highlightedItem = nullptr;
};

class MenuOverlay final : public QWidget
{
public:
    MenuOverlay(QString title, std::vector<MenuActionModel> actions, bool showIcons, QWidget *parent = nullptr)
        : QWidget(parent)
        , m_title(std::move(title))
        , m_actions(std::move(actions))
        , m_showIcons(showIcons)
    {
        setWindowFlag(Qt::FramelessWindowHint);
        setWindowFlag(Qt::Tool);
        setAttribute(Qt::WA_TranslucentBackground);
        setAttribute(Qt::WA_NoSystemBackground);
        setMouseTracking(true);
        setFocusPolicy(Qt::StrongFocus);
    }

    [[nodiscard]] QString selectedAction() const
    {
        return m_selectedAction;
    }

    std::function<void()> onFinished;

    void open()
    {
        const std::optional<CursorAnchor> anchor = resolveCursorAnchor();
        if (!anchor || anchor->screen == nullptr) {
            dismiss();
            return;
        }

        m_screen = anchor->screen;
        m_screenGeometry = m_screen->geometry();
        m_availableGeometry = m_screen->availableGeometry();
        m_cursorAnchor = anchor->position;
        appendPlacementLog(QStringLiteral("open anchor=(%1,%2) source=%3 screenGeom=(%4,%5 %6x%7) availableGeom=(%8,%9 %10x%11)")
                               .arg(m_cursorAnchor.x())
                               .arg(m_cursorAnchor.y())
                               .arg(anchor->source)
                               .arg(m_screenGeometry.x())
                               .arg(m_screenGeometry.y())
                               .arg(m_screenGeometry.width())
                               .arg(m_screenGeometry.height())
                               .arg(m_availableGeometry.x())
                               .arg(m_availableGeometry.y())
                               .arg(m_availableGeometry.width())
                               .arg(m_availableGeometry.height()));

        setGeometry(m_screenGeometry);
        winId();
        configureLayerShell();

        auto *rootPanel = new MenuPanel(&m_actions, m_showIcons, 0, this);
        rootPanel->onItemHovered = [this](MenuPanel *panel, MenuItemWidget *item) {
            handleItemHovered(panel, item);
        };
        rootPanel->onItemActivated = [this](MenuPanel *panel, MenuItemWidget *item) {
            handleItemActivated(panel, item);
        };
        m_panels.push_back(rootPanel);

        show();
        raise();
        activateWindow();
        QCoreApplication::processEvents();

        placeRootPanel(rootPanel);
        rootPanel->show();
        rootPanel->raise();
        setFocus(Qt::ActiveWindowFocusReason);

        if (MenuItemWidget *defaultItem = rootPanel->defaultItem()) {
            rootPanel->setHighlightedItem(defaultItem);
            if (!defaultItem->action()->submenu.empty()) {
                openSubmenu(rootPanel, defaultItem);
            }
        }
    }

protected:
    void mousePressEvent(QMouseEvent *event) override
    {
        QWidget::mousePressEvent(event);
        if (event->button() != Qt::LeftButton && event->button() != Qt::RightButton) {
            return;
        }

        for (auto *panel : m_panels) {
            if (panel->geometry().contains(event->position().toPoint())) {
                return;
            }
        }

        dismiss();
    }

    void keyPressEvent(QKeyEvent *event) override
    {
        if (event->key() == Qt::Key_Escape) {
            dismiss();
            return;
        }

        MenuPanel *panel = activePanel();
        MenuItemWidget *item = panel == nullptr ? nullptr : panel->highlightedItem();

        switch (event->key()) {
        case Qt::Key_Up:
            moveHighlight(panel, item, -1);
            return;
        case Qt::Key_Down:
            moveHighlight(panel, item, 1);
            return;
        case Qt::Key_Right:
            if (item != nullptr && !item->action()->submenu.empty()) {
                openSubmenu(panel, item);
                if (MenuPanel *submenu = activePanel()) {
                    if (MenuItemWidget *defaultItem = submenu->highlightedItem()) {
                        submenu->setHighlightedItem(defaultItem);
                    }
                }
                return;
            }
            break;
        case Qt::Key_Left:
            if (panel != nullptr && panel->level() > 0) {
                closePanelsFromLevel(panel->level());
                return;
            }
            break;
        case Qt::Key_Return:
        case Qt::Key_Enter:
        case Qt::Key_Space:
            if (panel != nullptr && item != nullptr) {
                handleItemActivated(panel, item);
                return;
            }
            break;
        default:
            break;
        }

        QWidget::keyPressEvent(event);
    }

    void closeEvent(QCloseEvent *event) override
    {
        QWidget::closeEvent(event);
        if (onFinished) {
            onFinished();
        }
    }

    [[nodiscard]] MenuPanel *activePanel() const
    {
        return m_panels.empty() ? nullptr : m_panels.back();
    }

    void moveHighlight(MenuPanel *panel, MenuItemWidget *item, int direction)
    {
        if (panel == nullptr) {
            return;
        }

        MenuItemWidget *target = item == nullptr
            ? (direction < 0 ? panel->lastItem() : panel->firstItem())
            : panel->nextItem(item, direction);
        if (target == nullptr) {
            return;
        }

        panel->setHighlightedItem(target);
        if (target->action()->submenu.empty()) {
            closePanelsFromLevel(panel->level() + 1);
        } else {
            openSubmenu(panel, target);
        }
    }

    void configureLayerShell()
    {
        if (auto *window = windowHandle()) {
            if (m_screen != nullptr) {
                window->setScreen(m_screen);
            }

            auto *layerShell = LayerShellQt::Window::get(window);
            if (layerShell != nullptr) {
                layerShell->setScope(QStringLiteral("kanyrun-menu"));
                layerShell->setLayer(LayerShellQt::Window::LayerOverlay);
                layerShell->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityExclusive);
                layerShell->setAnchors(LayerShellQt::Window::Anchors(
                    LayerShellQt::Window::AnchorTop |
                    LayerShellQt::Window::AnchorBottom |
                    LayerShellQt::Window::AnchorLeft |
                    LayerShellQt::Window::AnchorRight));
                layerShell->setMargins(QMargins());
                layerShell->setDesiredSize(QSize(0, 0));
                layerShell->setExclusiveZone(0);
                layerShell->setScreen(m_screen);
                layerShell->setActivateOnShow(true);
            }
        }
    }

    QPoint mapGlobalToOverlay(const QPoint &globalPoint) const
    {
        return globalPoint - geometry().topLeft();
    }

    void placePanel(MenuPanel *panel, const QPoint &globalTopLeft)
    {
        panel->adjustSize();
        const QPoint localTopLeft = mapGlobalToOverlay(globalTopLeft);
        const qreal devicePixelRatio = windowHandle() == nullptr ? 0.0 : windowHandle()->devicePixelRatio();
        appendPlacementLog(QStringLiteral("placePanel global=(%1,%2) overlayTopLeft=(%3,%4) overlaySize=%5x%6 dpr=%7 local=(%8,%9)")
                               .arg(globalTopLeft.x())
                               .arg(globalTopLeft.y())
                               .arg(geometry().x())
                               .arg(geometry().y())
                               .arg(width())
                               .arg(height())
                               .arg(devicePixelRatio, 0, 'f', 2)
                               .arg(localTopLeft.x())
                               .arg(localTopLeft.y()));
        panel->move(localTopLeft);
    }

    void placeRootPanel(MenuPanel *panel)
    {
        const QPoint globalPosition = clampMenuPosition(m_cursorAnchor, panel->sizeHint(), m_availableGeometry);
        placePanel(panel, globalPosition);
        QCoreApplication::processEvents();

        const QPoint correctedPosition = clampMenuPosition(mapToGlobal(panel->geometry().topLeft()), panel->frameGeometry().size(), m_availableGeometry);
        if (correctedPosition != mapToGlobal(panel->geometry().topLeft())) {
            appendPlacementLog(QStringLiteral("placeRootPanel corrected=(%1,%2)")
                                   .arg(correctedPosition.x())
                                   .arg(correctedPosition.y()));
            placePanel(panel, correctedPosition);
        }
    }
    void handleItemHovered(MenuPanel *panel, MenuItemWidget *item)
    {
        if (item->action()->submenu.empty()) {
            closePanelsFromLevel(panel->level() + 1);
            return;
        }
        openSubmenu(panel, item);
    }

    void handleItemActivated(MenuPanel *panel, MenuItemWidget *item)
    {
        if (!item->action()->submenu.empty()) {
            openSubmenu(panel, item);
            return;
        }
        m_selectedAction = item->action()->id;
        close();
    }

    void closePanelsFromLevel(int level)
    {
        while (static_cast<int>(m_panels.size()) > level) {
            MenuPanel *panel = m_panels.back();
            m_panels.pop_back();
            panel->hide();
            panel->deleteLater();
        }
    }

    void openSubmenu(MenuPanel *parentPanel, MenuItemWidget *item)
    {
        const int submenuLevel = parentPanel->level() + 1;
        closePanelsFromLevel(submenuLevel);

        auto *submenuPanel = new MenuPanel(&item->action()->submenu, m_showIcons, submenuLevel, this);
        submenuPanel->onItemHovered = [this](MenuPanel *panel, MenuItemWidget *submenuItem) {
            handleItemHovered(panel, submenuItem);
        };
        submenuPanel->onItemActivated = [this](MenuPanel *panel, MenuItemWidget *submenuItem) {
            handleItemActivated(panel, submenuItem);
        };
        m_panels.push_back(submenuPanel);

        submenuPanel->adjustSize();
        const QRect itemGlobalRect(item->mapToGlobal(QPoint(0, 0)), item->size());
        const QSize submenuSize = submenuPanel->sizeHint();
        const int submenuFirstItemTop = submenuPanel->firstItemTop();

        QPoint preferredPosition(itemGlobalRect.right(), itemGlobalRect.top() - submenuFirstItemTop);
        if (preferredPosition.x() + submenuSize.width() > m_availableGeometry.right() + 1) {
            preferredPosition.setX(itemGlobalRect.left() - submenuSize.width());
        }

        preferredPosition = clampMenuPosition(preferredPosition, submenuSize, m_availableGeometry);
        appendPlacementLog(QStringLiteral("openSubmenu parentItemTop=%1 parentItemHeight=%2 submenuFirstItemTop=%3 preferred=(%4,%5)")
                               .arg(itemGlobalRect.top())
                               .arg(itemGlobalRect.height())
                               .arg(submenuFirstItemTop)
                               .arg(preferredPosition.x())
                               .arg(preferredPosition.y()));
        placePanel(submenuPanel, preferredPosition);
        submenuPanel->show();
        submenuPanel->raise();

        if (MenuItemWidget *defaultItem = submenuPanel->defaultItem()) {
            submenuPanel->setHighlightedItem(defaultItem);
        }
    }

    void dismiss()
    {
        close();
    }

    QString m_title;
    std::vector<MenuActionModel> m_actions;
    QString m_selectedAction;
    QScreen *m_screen = nullptr;
    QRect m_screenGeometry;
    QRect m_availableGeometry;
    QPoint m_cursorAnchor;
    bool m_showIcons = true;
    std::vector<MenuPanel *> m_panels;
};

static QString runOverlayForPayload(const MenuPayloadModel &payload)
{
    if (payload.actions.empty()) {
        return {};
    }

    MenuOverlay overlay(payload.title, payload.actions, payload.showIcons);
    QEventLoop loop;
    overlay.onFinished = [&loop]() {
        loop.quit();
    };
    overlay.open();
    loop.exec();
    return overlay.selectedAction();
}

static int runOneShotUi()
{
    const auto raw = QByteArray::fromStdString(readStdin());
    QJsonParseError error;
    const auto document = QJsonDocument::fromJson(raw, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject()) {
        QTextStream(stderr) << "failed to parse menu payload: " << error.errorString() << Qt::endl;
        return 1;
    }

    const auto payload = parseMenuPayload(document.object());
    const QString selectedAction = runOverlayForPayload(payload);
    if (!selectedAction.isEmpty()) {
        QTextStream(stdout) << selectedAction << Qt::endl;
    }
    return 0;
}

static int runWarmUiLoop()
{
    std::string frame;
    while (readFramedStdin(&frame)) {
        const auto raw = QByteArray::fromStdString(frame);
        QJsonParseError error;
        const auto document = QJsonDocument::fromJson(raw, &error);
        if (error.error != QJsonParseError::NoError || !document.isObject()) {
            QTextStream(stderr) << "failed to parse menu payload: " << error.errorString() << Qt::endl;
            return 1;
        }

        const auto payload = parseMenuPayload(document.object());
        const QString selectedAction = runOverlayForPayload(payload);
        QTextStream out(stdout);
        out << selectedAction << Qt::endl;
        out.flush();
    }

    return 0;
}

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    app.setQuitOnLastWindowClosed(false);

    const QStringList arguments = QCoreApplication::arguments();
    if (arguments.contains(QStringLiteral("--print-cursor"))) {
        const bool allowFallback = !arguments.contains(QStringLiteral("--strict-cursor"));
        return printCursorAnchor(allowFallback);
    }
    if (arguments.contains(QStringLiteral("--serve-stdio"))) {
        return runWarmUiLoop();
    }

    return runOneShotUi();
}

#include "main.moc"
