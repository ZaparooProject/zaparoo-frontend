// Design-time only. Not compiled into the frontend.
// Mirrors the AppState persistence singleton exposed from Rust.
pragma Singleton
import QtQuick

QtObject {
    property string active_screen: "hub"
}
