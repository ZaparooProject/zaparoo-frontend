# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_library(zaparoo_ui_components STATIC)

# Format.qml is the one Zaparoo.Ui singleton -- every other file here is a
# plain, instantiable component. Mirrors Zaparoo.Theme's CMakeLists, which
# marks its own singletons the same way.
set_source_files_properties(Format.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(
    zaparoo_ui_components
    URI Zaparoo.Ui
    VERSION 1.0
    QML_FILES
        ActiveLabel.qml
        BootOverlay.qml
        DeferredAction.qml
        BrowseDetailPane.qml
        BrowseList.qml
        BrowseListDetailView.qml
        CardDivider.qml
        CommercialNoticeModal.qml
        ContextMenu.qml
        CrtCalibrationModal.qml
        DelayedLoadingIndicator.qml
        EmptySlot.qml
        FocusedMediaDetailController.qml
        Format.qml
        GameInfoModal.qml
        HeaderBar.qml
        LetterJumpModal.qml
        LoadingIndicator.qml
        ListPickerModal.qml
        LogUploadModal.qml
        Modal.qml
        PagedGrid.qml
        PageIndicator.qml
        PressableSurface.qml
        ProgressTrack.qml
        QrCodeModal.qml
        QrMatrix.qml
        RapidScrollIndicator.qml
        ScrapeSetupModal.qml
        ScreensaverOverlay.qml
        ScreenStateOverlay.qml
        ScrollingCaption.qml
        SelectionBar.qml
        SettingsField.qml
        SettingsSectionHeader.qml
        StatusIcon.qml
        StatusLine.qml
        Tile.qml
        TileLoader.qml
        TopStatusStrip.qml
)

target_link_libraries(
    zaparoo_ui_components
    PUBLIC Qt6::Quick
    PRIVATE Zaparoo::CompileOptions
)
