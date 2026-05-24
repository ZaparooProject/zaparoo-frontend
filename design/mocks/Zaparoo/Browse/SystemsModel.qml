// Design-time only. Not compiled into the frontend.
// Mirrors the API surface of the Rust SystemsModel singleton.
pragma Singleton

import QtQuick

ListModel {
    ListElement {
        system_id: "snes"
        name: "Super Nintendo"
    }
    ListElement {
        system_id: "megadrive"
        name: "Mega Drive"
    }
    ListElement {
        system_id: "nes"
        name: "Nintendo"
    }
    ListElement {
        system_id: "gameboy"
        name: "Game Boy"
    }

    property bool card_write_pending: false
    property string card_write_error: ""

    function set_category(_category: string): void {
    // No-op in the mock — full list stays visible regardless of category.
    }

    function system_id_at(index: int): string {
        return index >= 0 && index < count ? get(index).system_id : "";
    }

    function system_name_at(index: int): string {
        return index >= 0 && index < count ? get(index).name : "";
    }

    function write_card_at(_index: int): void {
    }
    function cancel_card_write(): void {
    }

    function index_for_system_id(id: string): int {
        for (let i = 0; i < count; ++i)
            if (get(i).system_id === id)
                return i;
        return -1;
    }
}
