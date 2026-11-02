// Spike S1 to S3 (see docs/ADR/0002-linux-desktop-integration.md).
//
// A throwaway measurement tool, not product code. It shows one small layer-shell
// window in the bottom-right corner and logs what the compositor does with it:
//   S1  where the window lands (observed by eye; Wayland clients cannot know it)
//   S2  whether it becomes active, and when, without any click
//   S3  whether it is deactivated when another window is clicked
// Typed characters are never logged, only how many keys arrived.

#include <LayerShellQt/Window>

#include <QCommandLineParser>
#include <QElapsedTimer>
#include <QEvent>
#include <QGuiApplication>
#include <QMargins>
#include <QQmlContext>
#include <QQuickView>
#include <QScreen>
#include <QSurfaceFormat>
#include <QTimer>

#include <cstdio>

namespace {

QElapsedTimer g_clock;

void log(const QString &what)
{
    std::fprintf(stdout, "[%6lld ms] %s\n", static_cast<long long>(g_clock.elapsed()), qPrintable(what));
    std::fflush(stdout);
}

QString describe(const QRect &r)
{
    return QStringLiteral("%1x%2+%3+%4").arg(r.width()).arg(r.height()).arg(r.x()).arg(r.y());
}

} // namespace

class Bridge : public QObject
{
    Q_OBJECT
public:
    int keys = 0;
    qint64 firstKeyMs = -1;

    Q_INVOKABLE void keyPressed()
    {
        ++keys;
        if (firstKeyMs < 0) {
            firstKeyMs = g_clock.elapsed();
            log(QStringLiteral("first key reached the input field (no click was needed if no mouse press was logged before)"));
        }
    }

    Q_INVOKABLE void escape() { Q_EMIT dismissRequested(QStringLiteral("Escape pressed")); }

Q_SIGNALS:
    void dismissRequested(const QString &reason);

protected:
    bool eventFilter(QObject *, QEvent *event) override
    {
        switch (event->type()) {
        case QEvent::MouseButtonPress:
            log(QStringLiteral("mouse press inside the window"));
            break;
        case QEvent::Enter:
            log(QStringLiteral("pointer entered the window"));
            break;
        case QEvent::Leave:
            log(QStringLiteral("pointer left the window"));
            break;
        default:
            break;
        }
        return false;
    }
};

int main(int argc, char **argv)
{
    g_clock.start();

    // LayerShellQt::Shell::useLayerShell() is deprecated and not needed since Qt 6.5.
    QSurfaceFormat format;
    format.setAlphaBufferSize(8); // transparent corners around the rounded rectangle
    QSurfaceFormat::setDefaultFormat(format);

    QGuiApplication app(argc, argv);

    QCommandLineParser parser;
    parser.addHelpOption();
    QCommandLineOption secondsOpt({QStringLiteral("s"), QStringLiteral("seconds")}, QStringLiteral("Close after N seconds."), QStringLiteral("N"), QStringLiteral("40"));
    QCommandLineOption modeOpt(QStringLiteral("interactivity"), QStringLiteral("Keyboard interactivity: ondemand, exclusive or none."), QStringLiteral("mode"), QStringLiteral("ondemand"));
    QCommandLineOption marginOpt(QStringLiteral("margin"), QStringLiteral("Margin from the screen corner, in pixels."), QStringLiteral("px"), QStringLiteral("16"));
    QCommandLineOption keepOpt(QStringLiteral("keep-on-focus-loss"), QStringLiteral("Do not close when the window loses focus."));
    parser.addOptions({secondsOpt, modeOpt, marginOpt, keepOpt});
    parser.process(app);

    const int seconds = parser.value(secondsOpt).toInt();
    const int margin = parser.value(marginOpt).toInt();
    const QString mode = parser.value(modeOpt);
    const bool closeOnFocusLoss = !parser.isSet(keepOpt);
    constexpr int width = 480;
    constexpr int height = 200;

    LayerShellQt::Window::KeyboardInteractivity interactivity = LayerShellQt::Window::KeyboardInteractivityOnDemand;
    if (mode == QLatin1String("exclusive")) {
        interactivity = LayerShellQt::Window::KeyboardInteractivityExclusive;
    } else if (mode == QLatin1String("none")) {
        interactivity = LayerShellQt::Window::KeyboardInteractivityNone;
    }

    log(QStringLiteral("Qt %1, platform=%2, session=%3, keyboard interactivity=%4, margin=%5, timeout=%6 s")
            .arg(QString::fromLatin1(qVersion()), app.platformName(), qEnvironmentVariable("XDG_SESSION_TYPE"), mode)
            .arg(margin)
            .arg(seconds));
    for (QScreen *screen : app.screens()) {
        log(QStringLiteral("screen %1: geometry=%2 available=%3 devicePixelRatio=%4")
                .arg(screen->name(), describe(screen->geometry()), describe(screen->availableGeometry()))
                .arg(screen->devicePixelRatio()));
    }

    Bridge bridge;
    QQuickView view;
    view.setColor(Qt::transparent);
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.rootContext()->setContextProperty(QStringLiteral("spike"), &bridge);
    view.setSource(QUrl(QStringLiteral("qrc:/Spike.qml")));
    if (view.status() != QQuickView::Ready) {
        log(QStringLiteral("QML failed to load"));
        return 2;
    }
    view.resize(width, height);
    view.installEventFilter(&bridge);

    // Layer-shell properties must be set before the window is shown.
    auto *shell = LayerShellQt::Window::get(&view);
    shell->setScope(QStringLiteral("popupai-spike-s1"));
    shell->setLayer(LayerShellQt::Window::LayerOverlay);
    shell->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorBottom | LayerShellQt::Window::AnchorRight));
    shell->setMargins(QMargins(0, 0, margin, margin));
    shell->setExclusiveZone(0); // stay inside the area left by panels
    shell->setKeyboardInteractivity(interactivity);
    shell->setDesiredSize(QSize(width, height));

    qint64 shownMs = -1;
    qint64 activeMs = -1;
    bool wasActive = false;
    int exitCode = 0;
    QString reason = QStringLiteral("timeout");

    auto finish = [&](const QString &why) {
        if (reason == QLatin1String("timeout") || why != QLatin1String("timeout")) {
            reason = why;
        }
        app.quit();
    };

    QObject::connect(&view, &QWindow::visibleChanged, &view, [&](bool visible) {
        if (visible && shownMs < 0) {
            shownMs = g_clock.elapsed();
        }
        log(QStringLiteral("visible=%1").arg(visible));
    });
    QObject::connect(&view, &QWindow::activeChanged, &view, [&]() {
        const bool active = view.isActive();
        log(QStringLiteral("active=%1").arg(active));
        if (active && activeMs < 0) {
            activeMs = g_clock.elapsed();
            log(QStringLiteral("became active %1 ms after being shown, without any click").arg(shownMs >= 0 ? activeMs - shownMs : -1));
        }
        if (active) {
            wasActive = true;
        } else if (wasActive && closeOnFocusLoss) {
            finish(QStringLiteral("focus lost (S3: outside click or focus moved away)"));
        }
    });
    QObject::connect(&view, &QWindow::focusObjectChanged, &view, [&](QObject *object) {
        log(QStringLiteral("focus object: %1").arg(object ? QString::fromLatin1(object->metaObject()->className()) : QStringLiteral("none")));
    });
    QObject::connect(&view, &QWindow::screenChanged, &view, [&](QScreen *screen) {
        log(QStringLiteral("window is on screen %1").arg(screen ? screen->name() : QStringLiteral("none")));
    });
    bool firstFrame = true;
    QObject::connect(&view, &QQuickWindow::frameSwapped, &view, [&]() {
        if (firstFrame) {
            firstFrame = false;
            log(QStringLiteral("first frame presented %1 ms after being shown").arg(shownMs >= 0 ? g_clock.elapsed() - shownMs : -1));
        }
    });
    QObject::connect(&bridge, &Bridge::dismissRequested, &app, [&](const QString &why) { finish(why); });

    QTimer::singleShot(3000, &view, [&]() {
        log(QStringLiteral("after 3 s: active=%1, size=%2x%3, screen=%4")
                .arg(view.isActive())
                .arg(view.width())
                .arg(view.height())
                .arg(view.screen() ? view.screen()->name() : QStringLiteral("none")));
        if (!view.isActive()) {
            log(QStringLiteral("NOTE: the window never became active, so no keyboard focus was granted without a click"));
        }
    });
    QTimer::singleShot(seconds * 1000, &app, [&]() { finish(QStringLiteral("timeout")); });

    log(QStringLiteral("showing the window"));
    view.show();
    const int status = app.exec();
    if (status != 0) {
        exitCode = status;
    }

    log(QStringLiteral("SUMMARY reason=\"%1\" shown_to_active_ms=%2 keys=%3 first_key_ms=%4 ever_active=%5")
            .arg(reason)
            .arg(activeMs >= 0 && shownMs >= 0 ? activeMs - shownMs : -1)
            .arg(bridge.keys)
            .arg(bridge.firstKeyMs)
            .arg(wasActive));
    return exitCode;
}

#include "main.moc"
