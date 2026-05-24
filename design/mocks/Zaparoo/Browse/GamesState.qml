// Design-time only. Not compiled into the frontend.
// Mirrors the GamesState persistence singleton exposed from Rust.
pragma Singleton
import QtQuick

QtObject {
    property string system_id: "snes"
    property string game_path: ""
}
