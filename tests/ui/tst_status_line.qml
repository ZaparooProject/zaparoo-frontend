// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Browse as Browse
import Zaparoo.Theme
import Zaparoo.Ui

// Covers the two components the header status-line redesign introduced:
// ProgressTrack (the segmented block bar that replaced CoreStatusPill's
// 4-dot spinner) and StatusLine (the full-row message ladder that
// replaced the pill itself). ProgressTrack's geometry invariants and
// StatusLine's priority-ladder resolution are the load-bearing behavior;
// see docs/style.md -> "Header status line" for the design writeup.
TestCase {
    id: testCase
    name: "UiStatusLine"

    Component {
        id: progressTrackComponent

        ProgressTrack {}
    }

    Component {
        id: statusLineComponent

        StatusLine {
            width: 500
        }
    }

    function init(): void {
        Browse.AppStatus.link_state = 2; // CONNECTED
        Browse.AppStatus.connection_state = 2; // READY
        Browse.AppStatus.last_error = "";

        Browse.MediaStatus.indexing = false;
        Browse.MediaStatus.optimizing = false;
        Browse.MediaStatus.paused = false;
        Browse.MediaStatus.current_step = 0;
        Browse.MediaStatus.total_steps = 0;
        Browse.MediaStatus.current_step_display = "";
        Browse.MediaStatus.total_files = 0;
        Browse.MediaStatus.total_media = 0;

        Browse.MediaStatus.scraping = false;
        Browse.MediaStatus.scrape_paused = false;
        Browse.MediaStatus.scrape_current_step = 0;
        Browse.MediaStatus.scrape_total_steps = 0;
        Browse.MediaStatus.scrape_current_step_display = "";
        Browse.MediaStatus.scrape_current_processed = 0;
        Browse.MediaStatus.scrape_current_total = 0;
        Browse.MediaStatus.scrape_matched = 0;
        Browse.MediaStatus.scrape_total = 0;
        Browse.MediaStatus.scrape_total_scraped = 0;
        Browse.MediaStatus.scrape_state = "";
        Browse.MediaStatus.scrape_error = "";

        Browse.StatusEvents.kind = "";
        Browse.StatusEvents.detail = "";
    }

    // ---- ProgressTrack ----

    // The whole point of moving off the old spinner's ad-hoc geometry:
    // the track's width must never depend on whether it's determinate,
    // indeterminate, idle, or mid-fill -- a continuous fill bar (or the
    // old pill's natural-width label) would drift, which is exactly the
    // reflow bug the footer/PageIndicator round already fixed elsewhere.
    function test_track_width_is_invariant_across_states(): void {
        const track = createTemporaryObject(progressTrackComponent, testCase, {
            "active": false
        });
        verify(track !== null);
        const idleWidth = track.width;

        track.active = true;
        track.totalKnown = true;
        track.currentStep = 3;
        track.totalSteps = 10;
        compare(track.width, idleWidth);

        track.currentStep = 10;
        compare(track.width, idleWidth);

        track.totalKnown = false;
        compare(track.width, idleWidth);

        track.paused = true;
        compare(track.width, idleWidth);
    }

    function test_track_filled_cells_follow_current_over_total_data(): list<var> {
        return [
            {
                tag: "just started",
                cellCount: 10,
                currentStep: 0,
                totalSteps: 10,
                expected: 0
            },
            {
                tag: "halfway",
                cellCount: 10,
                currentStep: 5,
                totalSteps: 10,
                expected: 5
            },
            {
                tag: "complete",
                cellCount: 10,
                currentStep: 10,
                totalSteps: 10,
                expected: 10
            },
            {
                tag: "rounds to nearest cell",
                cellCount: 12,
                currentStep: 1,
                totalSteps: 3,
                expected: 4
            }
        ];
    }

    function test_track_filled_cells_follow_current_over_total(data: var): void {
        const track = createTemporaryObject(progressTrackComponent, testCase, {
            "active": true,
            "totalKnown": true,
            "cellCount": data.cellCount,
            "currentStep": data.currentStep,
            "totalSteps": data.totalSteps
        });
        verify(track !== null);
        compare(track._filledCells, data.expected);
    }

    function test_track_indeterminate_ignores_step_counts(): void {
        const track = createTemporaryObject(progressTrackComponent, testCase, {
            "active": true,
            "totalKnown": false,
            "currentStep": 7,
            "totalSteps": 10
        });
        verify(track !== null);
        compare(track._filledCells, 0);
        // The marching cell (not a step-derived fraction) is what pulses
        // in this mode.
        track._marchIndex = 3;
        compare(track._pulseCellIndex, 3);
    }

    function test_track_paused_freezes_pulse_but_keeps_fill(): void {
        const track = createTemporaryObject(progressTrackComponent, testCase, {
            "active": true,
            "totalKnown": true,
            "cellCount": 10,
            "currentStep": 5,
            "totalSteps": 10
        });
        verify(track !== null);
        compare(track._pulsing, true);
        compare(track._filledCells, 5);

        track.paused = true;
        compare(track._pulsing, false);
        compare(track._filledCells, 5, "pausing must not lose the fill, only the flash");
    }

    function test_track_inactive_reports_no_fill_and_no_pulse(): void {
        const track = createTemporaryObject(progressTrackComponent, testCase, {
            "active": false,
            "totalKnown": true,
            "currentStep": 5,
            "totalSteps": 10
        });
        verify(track !== null);
        compare(track._filledCells, 0);
        compare(track._pulsing, false);
    }

    // ---- StatusLine ----

    function test_connection_problem_outranks_active_task(): void {
        Browse.AppStatus.link_state = 4; // UNREACHABLE
        Browse.MediaStatus.indexing = true;

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Disconnected");
        compare(line._showTrack, false, "a connection problem must suppress the task track too");
    }

    function test_core_error_includes_last_error_detail(): void {
        Browse.AppStatus.link_state = 2; // CONNECTED
        Browse.AppStatus.connection_state = 3; // ERROR
        Browse.AppStatus.last_error = "boom";

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Core error: boom");
    }

    function test_indexing_shows_track(): void {
        Browse.MediaStatus.indexing = true;
        Browse.MediaStatus.current_step = 3;
        Browse.MediaStatus.total_steps = 10;
        Browse.MediaStatus.current_step_display = "Super Nintendo";

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Indexing: Super Nintendo");
        compare(line._showTrack, true);

        const track = findChild(line, "statusLineTrack");
        verify(track !== null);
        compare(track.totalKnown, true);
        compare(track.paused, false);
        compare(track.currentStep, 3);
        compare(track.totalSteps, 10);
    }

    function test_indexing_paused_reports_game_running_and_freezes_track(): void {
        Browse.MediaStatus.indexing = true;
        Browse.MediaStatus.paused = true;
        Browse.MediaStatus.current_step = 3;
        Browse.MediaStatus.total_steps = 10;

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Indexing paused: game running");

        const track = findChild(line, "statusLineTrack");
        verify(track !== null);
        compare(track.paused, true);
    }

    function test_optimizing_is_indeterminate(): void {
        Browse.MediaStatus.optimizing = true;
        Browse.MediaStatus.current_step_display = "vacuum";

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Optimizing database: vacuum");

        const track = findChild(line, "statusLineTrack");
        verify(track !== null);
        compare(track.totalKnown, false);
    }

    function test_scraping_shows_track(): void {
        Browse.MediaStatus.scraping = true;
        Browse.MediaStatus.scrape_current_step = 3;
        Browse.MediaStatus.scrape_total_steps = 10;
        Browse.MediaStatus.scrape_current_step_display = "Super Nintendo";

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Scraping: Super Nintendo");

        const track = findChild(line, "statusLineTrack");
        verify(track !== null);
        compare(track.currentStep, 3);
        compare(track.totalSteps, 10);
    }

    // Core doesn't populate current_step_display until it has picked a
    // system/step to report -- most visible right at the start of a run,
    // and apparently the steady state for at least some optimize/vacuum
    // frames on real hardware. A bare "Indexing: " with nothing after the
    // colon reads as broken; each task falls back to an ellipsis instead.
    function test_task_label_falls_back_to_ellipsis_when_step_display_is_empty_data(): list<var> {
        return [
            {
                tag: "indexing",
                setup: function () {
                    Browse.MediaStatus.indexing = true;
                },
                expected: "Indexing…"
            },
            {
                tag: "optimizing",
                setup: function () {
                    Browse.MediaStatus.optimizing = true;
                },
                expected: "Optimizing database…"
            },
            {
                tag: "scraping",
                setup: function () {
                    Browse.MediaStatus.scraping = true;
                },
                expected: "Scraping…"
            }
        ];
    }

    function test_task_label_falls_back_to_ellipsis_when_step_display_is_empty(data: var): void {
        data.setup();

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, data.expected);
    }

    function test_scraping_paused_message(): void {
        Browse.MediaStatus.scraping = true;
        Browse.MediaStatus.scrape_paused = true;

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);
        compare(line._label, "Scraping paused: game running");
    }

    // The track is the rightmost element and a short label shrink-wraps
    // to its own content and sits flush against it, so slack space lands
    // to the left of the pair instead of opening a gap in the middle of
    // the message.
    function test_label_hugs_track_with_no_gap_when_short(): void {
        Browse.MediaStatus.indexing = true;
        Browse.MediaStatus.current_step = 1;
        Browse.MediaStatus.total_steps = 10;
        Browse.MediaStatus.current_step_display = "NES";

        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        const label = findChild(line, "statusLineLabel");
        const track = findChild(line, "statusLineTrack");
        verify(label !== null);
        verify(track !== null);
        compare(label.width, line._labelNaturalWidth);
        compare(Math.round(label.x + label.width + line._cellsSpacing), Math.round(track.x));
        compare(Math.round(track.x + track.width), Math.round(line.width), "the track must be flush against the row's own right edge");
    }

    // Regression test for a real layout bug caught during review:
    // ProgressTrack's own `width` is a fixed internal size that does not
    // collapse just because `visible` is false, so a label anchored
    // through the (hidden) track's geometry stayed stranded short of the
    // right edge even when no task was active. The reserve has to
    // collapse in `_trackReserve` itself.
    function test_label_reclaims_trailing_space_once_task_ends(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        Browse.MediaStatus.indexing = true;
        verify(line._trackReserve > 0, "an active task must reserve room for the track");

        Browse.MediaStatus.indexing = false;
        compare(line._showTrack, false);
        compare(line._trackReserve, 0, "a hidden track must not still eat into the label's available width");
    }

    function test_terminal_message_appears_on_busy_to_idle_edge(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        Browse.MediaStatus.total_files = 5432;
        Browse.MediaStatus.indexing = true;
        compare(line._terminalMessage, "", "starting a task must not itself produce a terminal message");
        Browse.MediaStatus.indexing = false;

        // Round 9: counts are locale-grouped via Format.count() (see
        // Format.qml), so the expected string tracks whatever the test
        // runner's own locale produces rather than assuming a fixed
        // separator -- this stays correct across locales/CI environments.
        const expected = "Indexed " + Format.count(5432) + " files";
        compare(line._terminalMessage, expected);
        compare(line._label, expected);
    }

    function test_terminal_message_clears_after_its_dwell(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true,
            "_terminalDwellMs": 30
        });
        verify(line !== null);

        Browse.MediaStatus.indexing = true;
        Browse.MediaStatus.indexing = false;
        compare(line._terminalMessage, "Indexing complete");
        tryCompare(line, "_terminalMessage", "", 1000);
    }

    function test_scrape_failure_becomes_a_terminal_message(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        Browse.MediaStatus.scraping = true;
        Browse.MediaStatus.scraping = false;
        Browse.MediaStatus.scrape_state = "failed";
        Browse.MediaStatus.scrape_error = "disk full";
        // Real Core sends every scrape field on the same terminal frame;
        // toggling `scraping` again fires the busy→idle edge with
        // `scrape_state`/`scrape_error` already in place, matching that.
        Browse.MediaStatus.scraping = true;
        Browse.MediaStatus.scraping = false;

        compare(line._terminalMessage, "Scrape failed: disk full");
    }

    function test_new_task_clears_a_pending_terminal_message(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        Browse.MediaStatus.indexing = true;
        Browse.MediaStatus.indexing = false;
        compare(line._terminalMessage, "Indexing complete");

        Browse.MediaStatus.indexing = true;
        compare(line._terminalMessage, "", "a fresh task must not leave a stale terminal message behind it");
        compare(line._showTrack, true);
    }

    function test_transient_event_shows_when_idle_and_drops_during_a_task(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        Browse.StatusEvents.kind = "playtime_warning";
        Browse.StatusEvents.detail = "4m58s";
        Browse.StatusEvents.sequence = Browse.StatusEvents.sequence + 1;
        compare(line._label, "Playtime limit in 4m58s");

        Browse.MediaStatus.indexing = true;
        Browse.StatusEvents.detail = "10s";
        Browse.StatusEvents.sequence = Browse.StatusEvents.sequence + 1;
        verify(line._label !== "Playtime limit in 10s", "an event must be dropped, not queued, while a task owns the line");
        compare(line._showTrack, true);
    }

    function test_transient_event_drops_while_a_terminal_message_is_shown(): void {
        const line = createTemporaryObject(statusLineComponent, testCase, {
            "mediaActivityEnabled": true
        });
        verify(line !== null);

        Browse.MediaStatus.indexing = true;
        Browse.MediaStatus.indexing = false;
        compare(line._terminalMessage, "Indexing complete");

        Browse.StatusEvents.kind = "playtime_warning";
        Browse.StatusEvents.detail = "4m58s";
        Browse.StatusEvents.sequence = Browse.StatusEvents.sequence + 1;
        compare(line._label, "Indexing complete", "the terminal message must not be preempted by an event");
    }
}
