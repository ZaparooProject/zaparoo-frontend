// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// QrMatrix repaint coverage.
//
// The regression this pins: `Browse.QrCode` is a single shared slot, and
// the matrix used to read it through a `row_at(row)` invokable. An
// invokable carries no notify signal, so each delegate's bits binding
// depended only on the delegate's own index, and the index only changes
// when the outer Repeater rebuilds on a changed `size`. Two payloads that
// happen to land on the same QR version leave `size` alone, so nothing
// rebuilt and the matrix kept painting the first content generated in the
// session -- the documentation code showing up in the log-upload modal,
// and one game's "Write with App" code showing for another.
//
// The fixtures are deliberately real-world lengths rather than synthetic
// ones: the shipped docs URL and a log-upload URL both land on QR version
// 3, which is exactly why the bug was invisible in normal use.
TestCase {
    id: testCase
    name: "UiQrMatrix"
    when: windowShown
    width: 640
    height: 480
    visible: true

    readonly property string docsUrl: "https://zaparoo.org/docs/frontend/"
    readonly property string logUrl: "https://logs.zaparoo.org/kf8s2xqv"

    QrMatrix {
        id: matrix
        maxQrPixels: 240
    }

    function _rowsSnapshot(): var {
        const out = [];
        for (let i = 0; i < Browse.QrCode.size; i++)
            out.push(Browse.QrCode.rows[i]);
        return out;
    }

    // Walks the live delegate tree rather than the model, so this fails if
    // the binding stops tracking the property even when the model is right.
    function _paintedRows(): var {
        const out = [];
        const stack = [matrix];
        while (stack.length > 0) {
            const node = stack.pop();
            if (node.bits !== undefined && node.row !== undefined)
                out[node.row] = node.bits;
            const kids = node.children;
            for (let i = 0; i < kids.length; i++)
                stack.push(kids[i]);
        }
        return out;
    }

    function cleanupTestCase(): void {
        Browse.QrCode.generate("");
    }

    function test_fixtures_share_a_qr_version(): void {
        Browse.QrCode.generate(testCase.docsUrl);
        const docsSize = Browse.QrCode.size;
        verify(docsSize > 0, "the docs URL must produce a matrix");
        Browse.QrCode.generate(testCase.logUrl);
        compare(Browse.QrCode.size, docsSize, "fixtures must share a QR version or this suite proves nothing");
    }

    function test_rows_change_when_a_same_size_payload_replaces_another(): void {
        Browse.QrCode.generate(testCase.docsUrl);
        const docsRows = _rowsSnapshot();
        Browse.QrCode.generate(testCase.logUrl);
        const logRows = _rowsSnapshot();
        compare(logRows.length, docsRows.length, "same QR version, so the row count must not move");
        verify(JSON.stringify(logRows) !== JSON.stringify(docsRows), "a new payload must replace the stored modules");
    }

    function test_painted_modules_follow_a_same_size_regenerate(): void {
        Browse.QrCode.generate(testCase.docsUrl);
        const docsPainted = _paintedRows();
        verify(docsPainted.length > 0, "the matrix must paint delegates for the docs code");
        Browse.QrCode.generate(testCase.logUrl);
        const logPainted = _paintedRows();
        compare(logPainted.length, docsPainted.length, "delegate count must not move across a same-version payload");
        verify(JSON.stringify(logPainted) !== JSON.stringify(docsPainted), "delegates must repaint when the payload changes under an unchanged size");
        compare(JSON.stringify(logPainted), JSON.stringify(_rowsSnapshot()), "painted modules must match the model exactly");
    }

    function test_failed_generation_clears_the_matrix(): void {
        Browse.QrCode.generate(testCase.docsUrl);
        verify(Browse.QrCode.size > 0, "precondition: a good payload is loaded");
        // Past the qrcode crate's byte-mode capacity at any version.
        Browse.QrCode.generate("z".repeat(8000));
        compare(Browse.QrCode.size, 0, "an ungeneratable payload must report no matrix");
        compare(_paintedRows().length, 0, "and must not leave the previous code painted");
    }
}
