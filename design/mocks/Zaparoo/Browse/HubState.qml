// Design-time only. Not compiled into the frontend.
// Mirrors the HubState persistence singleton exposed from Rust.
pragma Singleton
import QtQuick

QtObject {
    property string category: "Console"
    property string system_id: "snes"
    property string focus: "categories"
}
