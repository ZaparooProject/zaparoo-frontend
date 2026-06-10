# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_library(zaparoo_ui_components STATIC)

qt_add_qml_module(
    zaparoo_ui_components
    URI Zaparoo.Ui
    VERSION 1.0
    QML_FILES
        ActiveLabel.qml
        BootOverlay.qml
        BrowseDetailPane.qml
        BrowseList.qml
        BrowseListDetailView.qml
        CardDivider.qml
        CommercialNoticeModal.qml
        ContextMenu.qml
        CoreStatusPill.qml
        DelayedLoadingIndicator.qml
        FirstRunIndexModal.qml
        FocusedMediaDetailController.qml
        GameInfoModal.qml
        HeaderBar.qml
        LoadingIndicator.qml
        ListPickerModal.qml
        LogUploadModal.qml
        Modal.qml
        PagedGrid.qml
        QrCodeModal.qml
        RapidScrollIndicator.qml
        ScreensaverOverlay.qml
        ScreenStateOverlay.qml
        SettingsField.qml
        SettingsSectionHeader.qml
        StatusIcon.qml
        Tile.qml
        TileLoader.qml
        TopStatusStrip.qml
)

target_link_libraries(
    zaparoo_ui_components
    PUBLIC Qt6::Quick
    PRIVATE Zaparoo::CompileOptions
)
