// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use std::time::Instant;

use tracing::info;

#[derive(Debug)]
pub struct NavTiming {
    source: &'static str,
    started: Instant,
    request_done: Option<Instant>,
    apply_done: Option<Instant>,
    gate_started: Option<Instant>,
    cover_total: usize,
    cover_cache_hits: usize,
    cover_initial_pending: usize,
}

impl NavTiming {
    pub fn new(source: &'static str) -> Self {
        Self {
            source,
            started: Instant::now(),
            request_done: None,
            apply_done: None,
            gate_started: None,
            cover_total: 0,
            cover_cache_hits: 0,
            cover_initial_pending: 0,
        }
    }

    pub fn set_source(&mut self, source: &'static str) {
        self.source = source;
    }

    pub fn mark_request_done(&mut self) {
        if self.request_done.is_none() {
            self.request_done = Some(Instant::now());
        }
    }

    pub fn mark_apply_done(&mut self) {
        self.apply_done = Some(Instant::now());
    }

    pub fn start_gate(&mut self, total: usize, cache_hits: usize, initial_pending: usize) {
        self.gate_started = Some(Instant::now());
        self.cover_total = total;
        self.cover_cache_hits = cache_hits;
        self.cover_initial_pending = initial_pending;
    }

    pub fn log_release(self, screen: &'static str, reason: &'static str, pending_remaining: usize) {
        let now = Instant::now();
        let request_anchor = self.request_done.unwrap_or(self.started);
        let apply_anchor = self.apply_done.unwrap_or(request_anchor);
        let request_ms = request_anchor.duration_since(self.started).as_millis();
        let apply_ms = apply_anchor.duration_since(request_anchor).as_millis();
        let gate_ms = self.gate_started.map_or(0, |gate_started| {
            now.duration_since(gate_started).as_millis()
        });
        let total_ms = now.duration_since(self.started).as_millis();
        let fetched = self.cover_initial_pending.saturating_sub(pending_remaining);
        info!(
            screen,
            source = self.source,
            reason,
            request_ms,
            apply_ms,
            gate_ms,
            total_ms,
            cover_total = self.cover_total,
            cover_cache_hits = self.cover_cache_hits,
            cover_fetched = fetched,
            cover_pending = pending_remaining,
            "nav timing",
        );
    }
}
