// Design-time only. Not compiled into the frontend.
// Mirrors the API surface of the Rust CategoriesModel singleton so
// Qt Design Studio can render Main.qml without a running Rust plugin.
pragma Singleton
import QtQuick

ListModel {
    function category_at(index: int): string {
        return index >= 0 && index < count ? get(index).name : "";
    }

    function index_for_category(name: string): int {
        for (let i = 0; i < count; ++i)
            if (get(i).name === name) {
                return i;
            }
        return -1;
    }

    ListElement {
        name: "Arcade"
    }

    ListElement {
        name: "Console"
    }

    ListElement {
        name: "Computer"
    }

    ListElement {
        name: "Handheld"
    }
}
