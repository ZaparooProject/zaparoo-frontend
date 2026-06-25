// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include <QDir>
#include <QFile>
#include <QQmlEngine>
#include <QQuickStyle>
#include <QTemporaryDir>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickTest/quicktest.h>

Q_IMPORT_QML_PLUGIN(Zaparoo_AppPlugin)
Q_IMPORT_QML_PLUGIN(Zaparoo_Browse_plugin)
Q_IMPORT_QML_PLUGIN(Zaparoo_UiPlugin)
Q_IMPORT_QML_PLUGIN(Zaparoo_ThemePlugin)
#ifdef ZAPAROO_UPDATE_STATIC_QML_PLUGIN
Q_IMPORT_QML_PLUGIN(Zaparoo_UpdatePlugin)
#endif
#ifdef ZAPAROO_UPDATE_STATIC_NATIVE_PLUGIN
Q_IMPORT_QML_PLUGIN(Zaparoo_Update_Native_plugin)
#endif

extern "C" int zaparoo_rust_init();

// Initializes the Rust model globals (tokio runtime, client, catalog channel)
// before the QML engine is created. The WebSocket client will fail to connect
// (no server running) and models will be empty — fine for behavioural UI tests
// that don't depend on live catalog data.
class UiSetup : public QObject
{
    Q_OBJECT

  public slots:
    // NOLINTNEXTLINE(readability-convert-member-functions-to-static) — Qt slot, must be a member
    void applicationAvailable()
    {
        // Redirect persistent UI state to a throwaway temp file so the
        // Browse.AppState/HubState/GamesState setters don't clobber the
        // real ~/.config/zaparoo/state.toml when tests drive navigation.
        static QTemporaryDir tmpRoot(QDir::temp().filePath("zaparoo-ui-test-XXXXXX"));
        if (!tmpRoot.isValid())
        {
            qFatal("Failed to create a temporary UI test directory");
        }

        const QDir tmpRootDir(tmpRoot.path());
        const QString tmpState = tmpRootDir.filePath("state.toml");
        QFile::remove(tmpState);
        qputenv("ZAPAROO_STATE_FILE", tmpState.toUtf8());

        const QString tmpConfigHome = tmpRootDir.filePath("config");
        const QString tmpDataHome = tmpRootDir.filePath("data");
        QDir().mkpath(tmpConfigHome);
        QDir().mkpath(tmpDataHome);
        qputenv("XDG_CONFIG_HOME", tmpConfigHome.toUtf8());
        qputenv("XDG_DATA_HOME", tmpDataHome.toUtf8());

        // Match the real frontend's style selection. Also forces the test
        // binary to reference QQuickStyle, which keeps libQt6QuickControls2
        // on the link line under GNU ld --as-needed (cxx-qt-lib's
        // quickcontrols feature inside zaparoo_frontend_rs is the sole
        // other consumer and appears later on the command line).
        QQuickStyle::setStyle("Basic");
        zaparoo_rust_init();
    }

    // NOLINTNEXTLINE(readability-convert-member-functions-to-static) — Qt slot, must be a member
    void qmlEngineAvailable(QQmlEngine* engine)
    {
#ifdef ZAPAROO_UPDATE_RUNTIME_QML_IMPORT_PATH
        engine->addImportPath(QStringLiteral(ZAPAROO_UPDATE_RUNTIME_QML_IMPORT_PATH));
#else
        Q_UNUSED(engine)
#endif
    }
};

QUICK_TEST_MAIN_WITH_SETUP(zaparoo_ui, UiSetup)

#include "tst_main.moc"
