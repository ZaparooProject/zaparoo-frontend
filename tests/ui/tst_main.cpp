// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include <QDir>
#include <QFile>
#include <QQuickStyle>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickTest/quicktest.h>

Q_IMPORT_QML_PLUGIN(Zaparoo_AppPlugin)
Q_IMPORT_QML_PLUGIN(Zaparoo_Browse_plugin)
Q_IMPORT_QML_PLUGIN(Zaparoo_UiPlugin)
Q_IMPORT_QML_PLUGIN(Zaparoo_ThemePlugin)

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
        const QString tmpState = QDir::temp().filePath("zaparoo-test-state.toml");
        QFile::remove(tmpState);
        qputenv("ZAPAROO_STATE_FILE", tmpState.toUtf8());

        // Match the real frontend's style selection. Also forces the test
        // binary to reference QQuickStyle, which keeps libQt6QuickControls2
        // on the link line under GNU ld --as-needed (cxx-qt-lib's
        // quickcontrols feature inside zaparoo_frontend_rs is the sole
        // other consumer and appears later on the command line).
        QQuickStyle::setStyle("Basic");
        zaparoo_rust_init();
    }
};

QUICK_TEST_MAIN_WITH_SETUP(zaparoo_ui, UiSetup)

#include "tst_main.moc"
