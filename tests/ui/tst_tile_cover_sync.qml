// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// The direct regression test for tile pop-in.
//
// `TintedSvgImageProvider` is a plain synchronous QQuickImageProvider backed by
// the baked mask atlas, so a bundled icon can be decoded inline on the GUI
// thread and paint in the same frame as the tile that owns it. These tests
// assert that with `compare`, never `tryCompare` — the whole point is that no
// event loop turn passes between construction and Ready. A `tryCompare` here
// would pass just as happily against the async provider this replaced.
//
// The color-logo-style case is the guard on the other side: under
// `systemLogoStyle = "color"` a `systems/` key resolves to a plain qrc PNG, and
// a real PNG decode is 1-3 ms on ARM. Thirteen of those in one binding pass is
// a 13-40 ms stall — worse than the pop-in. That case must stay asynchronous.
TestCase {
    id: testCase

    name: "UiTileCoverSync"
    when: windowShown
    width: 640
    height: 480
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    // Minimal Tile host implementing the delegate parent contract. TileLoader
    // is a Loader, whose own incubation would confuse "same frame" — so the
    // contract is satisfied directly here and Tile is instantiated eagerly.
    Component {
        id: tileHost

        Item {
            property alias coverKey: host.coverKey
            property alias coverSynchronous: host.coverSynchronous

            width: 160
            height: 160

            Item {
                id: host

                property bool isSelected: false
                property bool isFocused: false
                property string name: "Host"
                property string coverKey: ""
                property bool coverSynchronous: true

                anchors.fill: parent

                Tile {
                    anchors.fill: parent
                }
            }
        }
    }

    function _makeTile(key: string, sync: bool): var {
        const item = createTemporaryObject(tileHost, testCase, {
            "coverKey": key,
            "coverSynchronous": sync
        });
        verify(item !== null);
        return item;
    }

    // The cover Image's own `parent` is PressableSurface, not the Tile root, so
    // walk up to whichever ancestor actually carries the metric.
    function _tileOf(cover: var): var {
        let node = cover;
        while (node && node._coverEverLoading === undefined)
            node = node.parent;
        verify(node !== null, "no ancestor of the cover carries _coverEverLoading");
        return node;
    }

    function test_synchronous_bundled_icon_is_ready_before_returning(): void {
        Resources.systemLogoStyle = "tinted";
        const cover = findChild(testCase._makeTile("icons/Settings", true), "tileCoverBase");
        verify(cover !== null);
        compare(cover.asynchronous, false, "a tinted key with coverSynchronous must load inline");
        compare(cover.status, Image.Ready, "the icon must be decoded before the tile's first frame");
    }

    function test_synchronous_system_logo_is_ready_before_returning(): void {
        Resources.systemLogoStyle = "tinted";
        const cover = findChild(testCase._makeTile("systems/SNES", true), "tileCoverBase");
        verify(cover !== null);
        compare(cover.asynchronous, false);
        compare(cover.status, Image.Ready);
    }

    function test_asynchronous_request_defers_to_the_reader_thread(): void {
        Resources.systemLogoStyle = "tinted";
        const cover = findChild(testCase._makeTile("icons/Settings", false), "tileCoverBase");
        verify(cover !== null);
        compare(cover.asynchronous, true);
        // The negative control for the two tests above: the same key, same
        // provider, same atlas entry, and yet the tile is still blank when
        // construction returns. If this ever reports Ready the synchronous
        // assertions have stopped proving anything.
        compare(cover.status, Image.Loading, "an async request must not have resolved yet");
        tryCompare(cover, "status", Image.Ready, 2000);
        // This is the one request in the suite guaranteed to observe a real
        // Loading edge: it is the first tinted-provider job dispatched to the
        // reader thread pool in the whole binary, so OS thread-spawn latency
        // reliably outlasts the synchronous JS continuation below. Later
        // requests against the same (fast, warmed-up) pool can resolve before
        // JS ever regains control, which made this assertion flaky when tried
        // against a fresh scheme/key pair further down the file -- see
        // test_ever_loading_records_whether_the_tile_was_ever_blank for the
        // heavier, deliberately-slow-decode alternative used there instead.
        compare(testCase._tileOf(cover)._coverEverLoading, true, "a request that was genuinely Loading must be recorded");
    }

    // Non-negotiable. A regression here is a 13-40 ms GUI-thread stall on every
    // Systems page flip, which is a worse symptom than the pop-in this change
    // removes.
    function test_color_style_system_png_never_loads_synchronously(): void {
        const originalStyle = Resources.systemLogoStyle;
        try {
            Resources.systemLogoStyle = "color";
            verify(!Resources.isTintedProviderKey("systems/SNES"), "color style must route SNES around the tint provider");
            const cover = findChild(testCase._makeTile("systems/SNES", true), "tileCoverBase");
            verify(cover !== null);
            compare(cover.asynchronous, true, "a color-style system PNG is a real decode and must stay off the GUI thread");
        } finally {
            Resources.systemLogoStyle = originalStyle;
        }
    }

    // Real cover art is out of scope for the synchronous path: it is fetched
    // from Core, arbitrarily large, and decoded from a compressed format.
    function test_real_cover_art_stays_asynchronous(): void {
        const cover = findChild(testCase._makeTile("media-image/whatever", true), "tileCoverBase");
        verify(cover !== null);
        compare(cover.asynchronous, true);
    }

    // `everLoading` is the metric the MiSTer acceptance criterion is stated in:
    // on Hub, Systems page 1, and the Settings root, 100% of tinted tiles must
    // log `everLoading=false`. It is only a valid instrument if it can tell the
    // two paths apart, so pin both directions here rather than trusting a log
    // line read off hardware.
    //
    // The async half deliberately uses a real PNG decode (systemLogoStyle
    // "color"), not the tinted provider. The tinted provider's LUT pass is
    // sub-millisecond, and once the reader thread pool is warm (any test after
    // the very first one in the binary) that request can resolve to Ready
    // before this function's JS ever regains control -- no observable Loading
    // edge, flag stays false, regardless of cache key. A qrc PNG decode is
    // real, slower work that reliably straddles the wait below.
    function test_ever_loading_records_whether_the_tile_was_ever_blank(): void {
        Resources.systemLogoStyle = "tinted";
        const syncCover = findChild(testCase._makeTile("icons/Settings", true), "tileCoverBase");
        compare(testCase._tileOf(syncCover)._coverEverLoading, false, "a synchronous decode never passes through Image.Loading");

        const originalStyle = Resources.systemLogoStyle;
        try {
            Resources.systemLogoStyle = "color";
            const asyncCover = findChild(testCase._makeTile("systems/SNES", false), "tileCoverBase");
            const asyncTile = testCase._tileOf(asyncCover);
            tryCompare(asyncCover, "status", Image.Ready, 2000);
            compare(asyncTile._coverEverLoading, true, "a real PNG decode off the GUI thread is blank for at least one frame");
        } finally {
            Resources.systemLogoStyle = originalStyle;
        }
    }

    // A recycled delegate must report on the cover it is showing now. Without
    // the per-source reset, one async load would poison every later
    // measurement on that tile and the acceptance criterion could never be met.
    // Same real-decode technique as above, for the same reason -- and a
    // different system id, since the test above already warmed SNES's PNG
    // into QQuickPixmapCache and a warm-cache hit is exactly the fast,
    // unobservable resolution this technique exists to avoid.
    function test_ever_loading_resets_when_the_cover_changes(): void {
        const originalStyle = Resources.systemLogoStyle;
        try {
            Resources.systemLogoStyle = "color";
            const host = testCase._makeTile("systems/Genesis", false);
            const cover = findChild(host, "tileCoverBase");
            const tile = testCase._tileOf(cover);
            tryCompare(cover, "status", Image.Ready, 2000);
            compare(tile._coverEverLoading, true);

            Resources.systemLogoStyle = "tinted";
            host.coverSynchronous = true;
            host.coverKey = "icons/Settings";
            compare(cover.status, Image.Ready);
            compare(tile._coverEverLoading, false, "the new cover resolved inline, so the stale reading must be cleared");
        } finally {
            Resources.systemLogoStyle = originalStyle;
        }
    }
}
