#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Bundle Qt shared libraries for LGPL-compliant desktop distribution.
# Run from the project root after a successful cmake --build.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="${PROJECT_ROOT}/build"
DEPLOY_DIR="${PROJECT_ROOT}/deploy/frontend"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

BINARY="${BUILD_DIR}/bin/frontend"
if [ ! -f "${BINARY}" ]; then
    error "Binary not found at ${BINARY}. Run 'cmake --build build' first."
fi

QT_QMAKE="$(which qmake6 2>/dev/null || which qmake 2>/dev/null || true)"
if [ -z "${QT_QMAKE}" ]; then
    error "qmake not found on PATH. Ensure Qt 6.7+ bin is in PATH."
fi
QT_BIN_DIR="$(dirname "${QT_QMAKE}")"
QT_ROOT="$(dirname "${QT_BIN_DIR}")"
info "Using Qt from: ${QT_ROOT}"

rm -rf "${DEPLOY_DIR}"
mkdir -p "${DEPLOY_DIR}/lib"
mkdir -p "${DEPLOY_DIR}/plugins/platforms"
mkdir -p "${DEPLOY_DIR}/plugins/imageformats"
mkdir -p "${DEPLOY_DIR}/qml"

info "Copying binary..."
cp "${BINARY}" "${DEPLOY_DIR}/"

QT_LIBS=(
    "libQt6Core.so.6"
    "libQt6Gui.so.6"
    "libQt6Network.so.6"
    "libQt6OpenGL.so.6"
    "libQt6Qml.so.6"
    "libQt6QmlModels.so.6"
    "libQt6QmlWorkerScript.so.6"
    "libQt6Quick.so.6"
    "libQt6QuickControls2.so.6"
    "libQt6QuickControls2Impl.so.6"
    "libQt6QuickLayouts.so.6"
    "libQt6QuickTemplates2.so.6"
    "libQt6DBus.so.6"
)

info "Copying Qt libraries..."
for lib in "${QT_LIBS[@]}"; do
    if [ -f "${QT_ROOT}/lib/${lib}" ]; then
        cp -L "${QT_ROOT}/lib/${lib}" "${DEPLOY_DIR}/lib/"
        info "  + ${lib}"
    else
        warn "  - ${lib} not found (may not be needed)"
    fi
done

info "Copying platform plugins..."
for plugin in linuxfb xcb eglfs; do
    plugin_file="${QT_ROOT}/plugins/platforms/libq${plugin}.so"
    if [ -f "$plugin_file" ]; then
        cp -L "$plugin_file" "${DEPLOY_DIR}/plugins/platforms/"
        info "  + libq${plugin}.so"
    fi
done

info "Copying image format plugins..."
for plugin in jpeg png; do
    plugin_file="${QT_ROOT}/plugins/imageformats/libq${plugin}.so"
    if [ -f "$plugin_file" ]; then
        cp -L "$plugin_file" "${DEPLOY_DIR}/plugins/imageformats/"
        info "  + libq${plugin}.so"
    fi
done

info "Copying QML modules..."
QML_MODULES=(
    "QtQuick"
    "QtQuick/Controls"
    "QtQuick/Controls/Basic"
    "QtQuick/Layouts"
    "QtQuick/Templates"
    "QtQuick/Window"
    "QtQml"
    "QtQml/Models"
    "QtQml/WorkerScript"
)

for module in "${QML_MODULES[@]}"; do
    src="${QT_ROOT}/qml/${module}"
    if [ -d "$src" ]; then
        mkdir -p "${DEPLOY_DIR}/qml/${module}"
        cp -rL "$src"/* "${DEPLOY_DIR}/qml/${module}/" 2>/dev/null || true
        info "  + ${module}"
    fi
done

info "Copying licenses..."
cp "${PROJECT_ROOT}/src/LICENSES/LGPLv3.txt" "${DEPLOY_DIR}/"
cp "${PROJECT_ROOT}/src/LICENSES/Qt-LGPL-NOTICE.txt" "${DEPLOY_DIR}/"
cp "${PROJECT_ROOT}/src/LICENSES/OFL.txt" "${DEPLOY_DIR}/"
cp "${PROJECT_ROOT}/src/LICENSES/PressStart2P-ATTRIBUTION.txt" "${DEPLOY_DIR}/"
cp "${PROJECT_ROOT}/COPYING" "${DEPLOY_DIR}/"

info "Creating frontend script..."
cat > "${DEPLOY_DIR}/run.sh" << 'EOF'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

export LD_LIBRARY_PATH="${SCRIPT_DIR}/lib:${LD_LIBRARY_PATH}"
export QT_PLUGIN_PATH="${SCRIPT_DIR}/plugins"
export QML2_IMPORT_PATH="${SCRIPT_DIR}/qml"

if [ -z "$DISPLAY" ] && [ -z "$WAYLAND_DISPLAY" ]; then
    export QT_QPA_PLATFORM="${QT_QPA_PLATFORM:-linuxfb:fb=/dev/fb0}"
    export QT_QUICK_BACKEND="${QT_QUICK_BACKEND:-software}"
fi

exec "${SCRIPT_DIR}/frontend" "$@"
EOF
chmod +x "${DEPLOY_DIR}/run.sh"

cat > "${DEPLOY_DIR}/qt.conf" << 'EOF'
[Paths]
Prefix = .
Libraries = lib
Plugins = plugins
Qml2Imports = qml
EOF

info "Deployment complete: ${DEPLOY_DIR}"
info ""
info "Contents:"
du -sh "${DEPLOY_DIR}"/*
info ""
info "To run: ${DEPLOY_DIR}/run.sh"
