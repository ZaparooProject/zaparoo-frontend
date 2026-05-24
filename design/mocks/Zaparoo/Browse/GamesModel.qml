// Design-time only. Not compiled into the frontend.
// Mirrors the API surface of the Rust GamesModel singleton.
pragma Singleton

import QtQuick

ListModel {
    ListElement {
        name: "Super Mario World"
        path: "/mock/smw.sfc"
    }
    ListElement {
        name: "Sonic the Hedgehog"
        path: "/mock/sonic.md"
    }
    ListElement {
        name: "The Legend of Zelda"
        path: "/mock/zelda.nes"
    }
    ListElement {
        name: "Tetris"
        path: "/mock/tetris.gb"
    }
    ListElement {
        name: "Street Fighter II"
        path: "/mock/sf2.zip"
    }

    property bool loading: false
    property string error_message: ""
    property bool has_next_page: false
    property string current_system_id: ""
    property bool card_write_pending: false
    property string card_write_error: ""

    function set_system(_system_id: string): void {
    }
    function launch_at(_index: int): void {
    }
    function write_card_at(_index: int): void {
    }
    function cancel_card_write(): void {
    }

    function name_at(index: int): string {
        return index >= 0 && index < count ? get(index).name : "";
    }

    function path_at(index: int): string {
        return index >= 0 && index < count ? get(index).path : "";
    }

    function index_for_game_path(path: string): int {
        for (let i = 0; i < count; ++i)
            if (get(i).path === path)
                return i;
        return -1;
    }
}
