// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Controllers and Bluetooth pairing: follows Core's device state into the
// header's low controller battery glyph and the Controls page's
// Controllers row, and drives the Controllers page that row opens (the
// connected cards, a search that renews itself while the page is open,
// the found list, pairing) and its Paired controllers page (Forget). Both
// are pages of the Settings panel. The rules live in
// `zaparoo_app::controllers`; this is the Slint adapter.
//
// Stability is the point of the page: cards keep their slots, the found
// list only ever appends, and nothing but the user's own input moves the
// cursor (`settings::live_update` keeps it on what it was on).

use slint::{ComponentHandle, SharedString};
use zaparoo_app::controllers::{
    self as rules, Battery, Label, PageLayout, PairSession, Sighting, Slots,
};
use zaparoo_app::settings::Row;
use zaparoo_core::device::{BluetoothDevice, Controller, ACTION_BLUETOOTH_PAIR};
use zaparoo_core::store::DeviceState;

use crate::router::{lock, Ctx, ListContext};
use crate::App;

/// What the frontend holds about controllers and Bluetooth.
#[derive(Debug, Default)]
pub struct Model {
    /// Core's device state as last heard.
    pub device: DeviceState,
    /// The Controllers page, while it or its Paired controllers page is
    /// open.
    session: Option<Session>,
    /// Bumped by every session, so the renewal of a closed one stops.
    tickets: u64,
    /// The device a Forget dialog is asking about: its address and name.
    pending_forget: Option<(String, String)>,
}

/// One visit to the Controllers page.
#[derive(Debug, Default)]
struct Session {
    ticket: u64,
    pair: PairSession,
    slots: Slots,
    overflow: usize,
    /// The Paired controllers page's order: as first seen, newly paired
    /// devices appended, forgotten ones dropped.
    paired: Vec<String>,
}

impl Model {
    fn controllers(&self) -> &[Controller] {
        self.device.controllers.as_deref().unwrap_or_default()
    }

    fn devices(&self) -> &[BluetoothDevice] {
        self.device
            .bluetooth_devices
            .as_ref()
            .map_or(&[], |s| s.devices.as_slice())
    }

    fn device(&self, id: &str) -> Option<&BluetoothDevice> {
        self.devices().iter().find(|d| d.id == id)
    }

    /// How many controllers Core lists as connected.
    pub(crate) fn connected(&self) -> usize {
        self.controllers().len()
    }

    /// Core can pair a controller for this client.
    pub(crate) fn can_pair(&self) -> bool {
        rules::pairing_available(
            self.device.capabilities.action(ACTION_BLUETOOTH_PAIR),
            self.device.bluetooth.as_ref().is_some_and(|b| b.present),
        )
    }

    /// There is an adapter, and it is switched off.
    pub(crate) fn bluetooth_off(&self) -> bool {
        let Some(adapter) = self.device.bluetooth.as_ref().filter(|b| b.present) else {
            return false;
        };
        adapter.powered == Some(false)
            || self
                .device
                .bluetooth_devices
                .as_ref()
                .is_some_and(|s| !s.powered)
    }

    /// Fill the registry's controller and Bluetooth inputs.
    pub(crate) fn fill_inputs(&self, inputs: &mut zaparoo_app::settings::Inputs) {
        inputs.controllers_listed = self.device.controllers.is_some();
        inputs.can_pair_controllers = self.can_pair();
        if let Some(session) = &self.session {
            inputs.bluetooth_found = session.pair.found().len();
            inputs.bluetooth_paired = session.paired.len();
        }
    }

    /// Fold Core's state into the open session: finds, the attempt, the
    /// cards' slots and the paired list's order.
    fn observe(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        let sightings = self.devices().iter().map(|d| Sighting {
            id: &d.id,
            name: rules::display_name(d.display_name.as_deref(), d.name.as_deref()),
            paired: d.paired,
            connected: d.connected,
        });
        let attempt = self
            .device
            .bluetooth_devices
            .as_ref()
            .and_then(|s| s.pairing.as_ref());
        session.pair.observe(
            sightings,
            attempt.map(|a| rules::Attempt {
                id: &a.id,
                number: a.attempt,
                state: &a.state,
                error: a.error.as_deref(),
            }),
            self.device.bluetooth_devices.as_ref().map(|s| s.scanning),
        );
        let connected: Vec<&str> = self.controllers().iter().map(|c| c.id.as_str()).collect();
        session.overflow = session.slots.place(&connected);
        let paired: Vec<&str> = self
            .devices()
            .iter()
            .filter(|d| d.paired)
            .map(|d| d.id.as_str())
            .collect();
        // An adapter that is off or gone lists nothing, which says nothing
        // about what is paired: BlueZ lists them again once it is back. Only
        // a powered adapter's list drops a row, so the cursor on one stays
        // put through a dongle reset.
        let listing = self
            .device
            .bluetooth_devices
            .as_ref()
            .is_some_and(|s| s.powered);
        if listing {
            session.paired.retain(|id| paired.contains(&id.as_str()));
        }
        for id in paired {
            if !session.paired.iter().any(|p| p == id) {
                session.paired.push(id.to_string());
            }
        }
        self.session = Some(session);
    }

    /// The Bluetooth address of a connected controller, where Core has
    /// matched it to a device.
    fn address_of(&self, controller: &Controller) -> Option<&str> {
        self.devices()
            .iter()
            .find(|d| d.controller_id.as_deref() == Some(controller.id.as_str()))
            .map(|d| d.id.as_str())
    }

    /// The controller in each slot, if any.
    fn slotted(&self) -> Vec<Option<&Controller>> {
        let Some(session) = &self.session else {
            return Vec::new();
        };
        session
            .slots
            .ids()
            .iter()
            .map(|id| {
                id.as_deref()
                    .and_then(|id| self.controllers().iter().find(|c| c.id == id))
            })
            .collect()
    }

    /// Names for everything the Controllers page shows at once, the cards
    /// then the found list, so two of the same name anywhere on it are
    /// told apart.
    fn page_labels(&self) -> (Vec<Label>, Vec<Label>) {
        let Some(session) = &self.session else {
            return (Vec::new(), Vec::new());
        };
        let slotted = self.slotted();
        let mut entries: Vec<(Option<&str>, Option<&str>)> = slotted
            .iter()
            .map(|c| {
                c.map_or((None, None), |c| {
                    (
                        rules::display_name(c.display_name.as_deref(), c.name.as_deref()),
                        self.address_of(c),
                    )
                })
            })
            .collect();
        entries.extend(
            session
                .pair
                .found()
                .iter()
                .map(|f| (f.name.as_deref(), Some(f.id.as_str()))),
        );
        let mut labels = rules::labels(&entries);
        let found = labels.split_off(slotted.len());
        (labels, found)
    }

    fn paired_labels(&self) -> Vec<Label> {
        let Some(session) = &self.session else {
            return Vec::new();
        };
        let entries: Vec<(Option<&str>, Option<&str>)> = session
            .paired
            .iter()
            .map(|id| {
                let name = self.device(id).and_then(|d| {
                    rules::display_name(d.display_name.as_deref(), d.name.as_deref())
                });
                (name, Some(id.as_str()))
            })
            .collect();
        rules::labels(&entries)
    }

    /// What a row stands for, stable while the lists around it change: a
    /// device's own address, otherwise the row id.
    pub(crate) fn row_key(&self, row: Row) -> String {
        let Some(session) = &self.session else {
            return row.id().to_string();
        };
        match row {
            Row::Item {
                id: "foundDevice",
                index,
                ..
            } => session
                .pair
                .found()
                .get(index)
                .map_or_else(String::new, |f| format!("found:{}", f.id)),
            Row::Item {
                id: "pairedDevice",
                index,
                ..
            } => session
                .paired
                .get(index)
                .map_or_else(String::new, |id| format!("paired:{id}")),
            _ => row.id().to_string(),
        }
    }

    /// Fill a device row's name and state.
    pub(crate) fn fill_row(&self, row: Row, out: &mut crate::SettingsRow) {
        let (Some(index), Some(session)) = (row.item(), &self.session) else {
            return;
        };
        out.item = i32::try_from(index).unwrap_or(0);
        let put = |out: &mut crate::SettingsRow, label: Option<&Label>| {
            let label = label.cloned().unwrap_or_default();
            out.value_name = SharedString::from(label.name.unwrap_or_default());
            out.name_tail = SharedString::from(label.tail.unwrap_or_default());
        };
        match row.id() {
            "foundDevice" => {
                let Some(state) = session.pair.state(index) else {
                    return;
                };
                put(out, self.page_labels().1.get(index));
                out.device = state.into();
                if let rules::DeviceState::Failed(error) = state {
                    out.pair_error = error.into();
                }
            }
            "pairedDevice" => {
                let Some(id) = session.paired.get(index) else {
                    return;
                };
                put(out, self.paired_labels().get(index));
                out.device = if self.device(id).is_some_and(|d| d.connected) {
                    crate::DeviceValue::Connected
                } else {
                    crate::DeviceValue::NotConnected
                };
            }
            _ => {}
        }
    }

    fn search(&self) -> crate::PairSearch {
        if self.bluetooth_off() {
            return crate::PairSearch::AdapterOff;
        }
        match self.session.as_ref().map(|s| s.pair.search()) {
            Some(rules::Search::Off) => crate::PairSearch::AdapterOff,
            Some(rules::Search::Failed) => crate::PairSearch::Failed,
            Some(rules::Search::Pairing) => crate::PairSearch::Pairing,
            _ => crate::PairSearch::Searching,
        }
    }

    /// The Connected area's slots, as the view draws them.
    fn slots(&self) -> Vec<crate::ControllerSlot> {
        let (labels, _) = self.page_labels();
        self.slotted()
            .into_iter()
            .enumerate()
            .map(|(i, controller)| {
                let Some(controller) = controller else {
                    return crate::ControllerSlot::default();
                };
                let label = labels.get(i).cloned().unwrap_or_default();
                let battery = battery(controller);
                let reading = rules::reading(battery.as_ref(), &controller.connection);
                crate::ControllerSlot {
                    filled: true,
                    name: label.name.unwrap_or_default().into(),
                    tail: label.tail.unwrap_or_default().into(),
                    reading: reading.into(),
                    percent: match reading {
                        rules::Reading::Percent(p) | rules::Reading::Charging(Some(p)) => {
                            i32::from(p)
                        }
                        _ => 0,
                    },
                    bar: battery
                        .as_ref()
                        .and_then(rules::bar_percent)
                        .map_or(-1, i32::from),
                    low: battery.as_ref().is_some_and(Battery::is_low),
                    charging: matches!(reading, rules::Reading::Charging(_)),
                }
            })
            .collect()
    }
}

fn battery(controller: &Controller) -> Option<Battery> {
    controller
        .battery
        .as_ref()
        .map(|b| Battery::new(b.percent, &b.level, b.charge_state.as_deref()))
}

/// Whether some controller is running flat with nothing charging it.
fn any_low(controllers: &[Controller]) -> bool {
    let batteries: Vec<Battery> = controllers.iter().filter_map(battery).collect();
    rules::any_low(&batteries)
}

/// The header's controller batteries, one glyph each.
fn header_pads(controllers: &[Controller]) -> Vec<crate::HeaderPad> {
    let batteries: Vec<Option<Battery>> = controllers.iter().map(battery).collect();
    rules::header_pads(batteries.iter().map(Option::as_ref))
        .into_iter()
        .map(|pad| crate::HeaderPad {
            fill: match pad.fill {
                rules::Fill::Empty => crate::PadFill::Empty,
                rules::Fill::Low => crate::PadFill::Low,
                rules::Fill::Half => crate::PadFill::Half,
                rules::Fill::Full => crate::PadFill::Full,
            },
            charging: pad.charging,
            low: pad.low,
            marks_before: i32::try_from(pad.marks_before).unwrap_or(0),
        })
        .collect()
}

/// Push a page layout into a view's figures.
pub(crate) fn push_layout(view: &crate::ControllersView, l: &PageLayout) {
    view.set_compact(l.compact);
    view.set_slots_y(l.slots_y as f32);
    view.set_slot_columns(l.slot_columns);
    view.set_slot_width(l.slot_width as f32);
    view.set_slot_height(l.slot_height as f32);
    view.set_slot_gap(l.slot_gap as f32);
    view.set_status_y(l.status_y as f32);
    view.set_status_height(l.status_height as f32);
    view.set_list_y(l.list_y as f32);
    view.set_row_height(l.row_height as f32);
    view.set_visible_rows(l.visible_rows);
    view.set_paired_y(l.paired_y as f32);
}

/// Publish the Controllers page with the cursor on row `index`: its
/// cards, the search's status, and the found list scrolled only as far
/// as the cursor needs.
pub(crate) fn publish(ctx: &Ctx, app: &App, index: usize) {
    let layout = rules::page_layout(&crate::router::output_scene(app).inputs());
    let view = app.global::<crate::ControllersView>();
    push_layout(&view, &layout);
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.controllers;
    let visible = usize::try_from(layout.visible_rows).unwrap_or(0);
    let top = model.session.as_mut().map_or(0, |session| {
        if index < session.pair.found().len() {
            session.pair.follow(index, visible);
        }
        session.pair.top()
    });
    view.set_top(i32::try_from(top).unwrap_or(0));
    view.set_connected(i32::try_from(model.connected()).unwrap_or(0));
    view.set_overflow(
        model
            .session
            .as_ref()
            .map_or(0, |s| i32::try_from(s.overflow).unwrap_or(0)),
    );
    view.set_can_pair(model.can_pair());
    view.set_search(model.search());
    let slots = model.slots();
    drop(shared);
    crate::view_model::publish(&view.get_slots(), slots, |slots| view.set_slots(slots));
}

/// The CRT head's own layout for the page, from the primary's state: the
/// same top row unless its shorter list needs the cursor brought in.
#[cfg(feature = "mister")]
pub(crate) fn mirror_layout(primary: &App, crt: &App, inputs: &zaparoo_app::sizing::Inputs) {
    let layout = rules::page_layout(inputs);
    let view = crt.global::<crate::ControllersView>();
    push_layout(&view, &layout);
    let source = primary.global::<crate::ControllersView>();
    let index = usize::try_from(primary.global::<crate::SettingsView>().get_index()).unwrap_or(0);
    let top = usize::try_from(source.get_top()).unwrap_or(0);
    let visible = usize::try_from(layout.visible_rows).unwrap_or(0);
    let found = {
        use slint::Model as _;
        primary
            .global::<crate::SettingsView>()
            .get_rows()
            .iter()
            .filter(|row| row.id == "foundDevice")
            .count()
    };
    let top = if index < found {
        rules::keep_in_view(top, index, visible)
    } else {
        top
    };
    view.set_top(i32::try_from(top).unwrap_or(0));
}

/// Follow Core's device state for the life of the app.
pub fn bind(ctx: &Ctx, app: &App) {
    let mut rx = ctx.store.device().subscribe();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            let state = rx.borrow_and_update().clone();
            let ctx2 = ctx.clone();
            if weak
                .upgrade_in_event_loop(move |app| changed(&ctx2, &app, state))
                .is_err()
            {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
}

pub(crate) fn changed(ctx: &Ctx, app: &App, state: DeviceState) {
    let controllers = state.controllers.as_deref().unwrap_or_default();
    let shell = app.global::<crate::Shell>();
    shell.set_controller_battery_low(any_low(controllers));
    shell.set_controller_batteries(slint::ModelRc::new(slint::VecModel::from(header_pads(
        controllers,
    ))));
    let mut switched_on = false;
    crate::settings::live_update(ctx, app, |model| {
        let was_off = model.bluetooth_off();
        model.device = state;
        model.observe();
        switched_on = was_off && !model.bluetooth_off() && model.session.is_some();
    });
    // The adapter came on while the page is open: search now rather than
    // at the next renewal.
    if switched_on {
        start_scan(ctx, app, false);
    }
    schedule_retry(ctx, app);
    search_if_due(ctx, app);
}

/// Run a `DeviceResource` request off the UI thread and come back with
/// its outcome.
fn request<T, F, Fut>(
    ctx: &Ctx,
    app: &App,
    call: F,
    done: impl FnOnce(&Ctx, &App, Result<T, zaparoo_core::client::ClientError>) + Send + 'static,
) where
    T: Send + 'static,
    F: FnOnce(std::sync::Arc<zaparoo_core::store::DeviceResource>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, zaparoo_core::client::ClientError>> + Send,
{
    let resource = ctx.store.device();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        let result = call(resource).await;
        let _ = weak.upgrade_in_event_loop(move |app| done(&ctx, &app, result));
    });
}

/// The Controls page's Controllers row: open the page, and start the
/// search where Core can pair.
pub(crate) fn open_controllers(ctx: &Ctx, app: &App) {
    let (ticket, can_pair) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.controllers;
        model.tickets += 1;
        model.session = Some(Session {
            ticket: model.tickets,
            ..Session::default()
        });
        model.observe();
        (model.tickets, model.can_pair())
    };
    crate::settings::navigate_page(ctx, app, crate::SettingsPage::Controllers);
    if can_pair {
        start_scan(ctx, app, false);
        renew_while_open(ctx, app, ticket);
    }
}

/// Ask Core to search, or to keep searching (`renewal`). A repeat request
/// extends the running search and keeps what it found, so the page never
/// sees the search end.
fn start_scan(ctx: &Ctx, app: &App, renewal: bool) {
    if let Some(session) = lock(&ctx.shared).controllers.session.as_mut() {
        session.pair.search_requested(renewal);
    }
    send_scan(ctx, app);
}

/// Search again when the page's rules say so: Core's search ended under
/// the page, a pairing attempt finished, or Core came back.
fn search_if_due(ctx: &Ctx, app: &App) {
    let due = lock(&ctx.shared)
        .controllers
        .session
        .as_mut()
        .is_some_and(|s| s.pair.take_search());
    if due {
        send_scan(ctx, app);
    }
}

/// A refusal, or an adapter that does not start, says so on the status
/// line, never in an alert: the renewal tries again on its own.
fn send_scan(ctx: &Ctx, app: &App) {
    request(
        ctx,
        app,
        |device| async move { device.scan_start(rules::SEARCH_SECONDS).await },
        |ctx, app, result| {
            let current = ctx.store.device().current();
            match result {
                Ok(()) => crate::settings::live_update(ctx, app, move |model| {
                    let scanning = current
                        .bluetooth_devices
                        .as_ref()
                        .is_some_and(|s| s.scanning);
                    model.device = current;
                    if let Some(session) = model.session.as_mut() {
                        session.pair.search_answered(scanning);
                    }
                    model.observe();
                }),
                Err(err) => {
                    tracing::warn!("bluetooth scan failed: {}", err.message);
                    let error = rules::PairError::from_request_error(err.category.as_deref());
                    if let Some(session) = lock(&ctx.shared).controllers.session.as_mut() {
                        session.pair.search_refused(error);
                    }
                    crate::settings::refresh(ctx, app);
                }
            }
        },
    );
}

/// Keep the search going for as long as this session's page is open.
fn renew_while_open(ctx: &Ctx, app: &App, ticket: u64) {
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(rules::RENEW_SECONDS)).await;
            let open = lock(&ctx.shared)
                .controllers
                .session
                .as_ref()
                .is_some_and(|s| s.ticket == ticket);
            if !open {
                return;
            }
            let ctx2 = ctx.clone();
            if weak
                .upgrade_in_event_loop(move |app| renew(&ctx2, &app, ticket))
                .is_err()
            {
                return;
            }
        }
    });
}

fn renew(ctx: &Ctx, app: &App, ticket: u64) {
    let current = lock(&ctx.shared)
        .controllers
        .session
        .as_ref()
        .is_some_and(|s| s.ticket == ticket);
    if !current {
        return;
    }
    // Settings was left some other way (a Hub shortcut, a launch): the
    // page is gone, and so is its search.
    let page = app.global::<crate::SettingsView>().get_page();
    let on_page = app.global::<crate::Shell>().get_active_screen() == crate::Screen::Settings
        && matches!(
            page,
            crate::SettingsPage::Controllers | crate::SettingsPage::PairedControllers
        );
    if !on_page {
        end_session(ctx, app);
        return;
    }
    // No adapter, or one that is off, has nothing to renew: an adapter
    // switching on starts the search itself. Nor does a pairing attempt,
    // which searches again when it ends.
    let renew = {
        let shared = lock(&ctx.shared);
        let model = &shared.controllers;
        model.can_pair()
            && !model.bluetooth_off()
            && !model.session.as_ref().is_some_and(|s| s.pair.busy())
    };
    if renew {
        start_scan(ctx, app, true);
    }
}

/// Close the page's session and stop its search.
fn end_session(ctx: &Ctx, app: &App) {
    let scanning = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.controllers;
        model.session = None;
        model
            .device
            .bluetooth_devices
            .as_ref()
            .is_some_and(|s| s.scanning)
    };
    if scanning {
        request(
            ctx,
            app,
            |device| async move { device.scan_stop().await },
            |_, _, result| {
                // Core stops on its own timeout anyway.
                if let Err(err) = result {
                    tracing::debug!("bluetooth scan stop failed: {}", err.message);
                }
            },
        );
    }
}

/// Back out of the Controllers page: stop searching and return to
/// Controls, on the Controllers row.
pub(crate) fn leave_controllers(ctx: &Ctx, app: &App) {
    end_session(ctx, app);
    crate::settings::return_to_row(ctx, app, crate::SettingsPage::Controls, "controllers");
}

/// The Paired controllers row. The search goes on meanwhile: this is a
/// look inside the page, not a way out of it.
pub(crate) fn open_paired(ctx: &Ctx, app: &App) {
    crate::settings::navigate_page(ctx, app, crate::SettingsPage::PairedControllers);
}

/// Back from Paired controllers, onto the row that opened it.
pub(crate) fn leave_paired(ctx: &Ctx, app: &App) {
    crate::settings::return_to_row(
        ctx,
        app,
        crate::SettingsPage::Controllers,
        "pairedControllers",
    );
}

/// Accept on a found device: pair it, or stop trying on the one being
/// paired.
pub(crate) fn accept_device(ctx: &Ctx, app: &App, index: usize) {
    let id = {
        let mut shared = lock(&ctx.shared);
        let Some(session) = shared.controllers.session.as_mut() else {
            return;
        };
        if session.pair.state(index) == Some(rules::DeviceState::Pairing) {
            session.pair.stop_pairing();
            drop(shared);
            crate::settings::refresh(ctx, app);
            search_if_due(ctx, app);
            return;
        }
        let Some(id) = session.pair.pair_requested(index) else {
            return;
        };
        id
    };
    crate::settings::refresh(ctx, app);
    send_pair(ctx, app, id);
}

/// After a failed attempt the page asks again, a moment later, for as long
/// as it is still trying that device.
fn schedule_retry(ctx: &Ctx, app: &App) {
    let (ticket, id) = {
        let mut shared = lock(&ctx.shared);
        let Some(session) = shared.controllers.session.as_mut() else {
            return;
        };
        let Some(id) = session.pair.take_retry() else {
            return;
        };
        (session.ticket, id)
    };
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(rules::RETRY_SECONDS)).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let trying = lock(&ctx.shared)
                .controllers
                .session
                .as_ref()
                .is_some_and(|s| s.ticket == ticket && s.pair.pairing(&id));
            if trying {
                send_pair(&ctx, &app, id);
            }
        });
    });
}

fn send_pair(ctx: &Ctx, app: &App, id: String) {
    request(
        ctx,
        app,
        move |device| async move { device.pair(&id).await },
        |ctx, app, result| {
            let err = match result {
                Ok(number) => {
                    // Core's state may already hold this attempt's outcome:
                    // fold it again now that the attempt is known to be ours.
                    let current = ctx.store.device().current();
                    crate::settings::live_update(ctx, app, move |model| {
                        model.device = current;
                        if let Some(session) = model.session.as_mut() {
                            session.pair.pair_accepted(number);
                        }
                        model.observe();
                    });
                    schedule_retry(ctx, app);
                    search_if_due(ctx, app);
                    return;
                }
                Err(err) => err,
            };
            tracing::warn!("bluetooth pair failed: {}", err.message);
            if let Some(session) = lock(&ctx.shared).controllers.session.as_mut() {
                session
                    .pair
                    .pair_refused(rules::PairError::from_request_error(
                        err.category.as_deref(),
                    ));
            }
            crate::settings::refresh(ctx, app);
            schedule_retry(ctx, app);
            search_if_due(ctx, app);
        },
    );
}

/// Options on a paired device: Forget.
pub(crate) fn open_options(ctx: &Ctx, app: &App, index: usize) {
    let id = {
        let shared = lock(&ctx.shared);
        let Some(id) = shared
            .controllers
            .session
            .as_ref()
            .and_then(|s| s.paired.get(index))
        else {
            return;
        };
        id.clone()
    };
    crate::router::present_list(
        ctx,
        app,
        ListContext::BluetoothDeviceOptions(id),
        "title:options",
        vec![crate::router::menu_row_keyed(
            "bluetooth_forget",
            "bluetooth_forget",
            "",
        )],
    );
}

/// Forget was picked: ask first, since the controller cannot reconnect
/// until it is paired again, and it may be the one in the user's hands.
pub(crate) fn forget_picked(ctx: &Ctx, app: &App, id: &str) {
    let (name, tail) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.controllers;
        let Some(index) = model
            .session
            .as_ref()
            .and_then(|s| s.paired.iter().position(|p| p == id))
        else {
            return;
        };
        let label = model
            .paired_labels()
            .get(index)
            .cloned()
            .unwrap_or_default();
        // An unnamed one goes by its address, which is its own text, not
        // ours to translate.
        let name = label.name.unwrap_or_else(|| id.to_string());
        // Two of the same name are told apart the way the page tells them
        // apart: by the tail of their address.
        let tail = label.tail.unwrap_or_default();
        model.pending_forget = Some((id.to_string(), name.clone()));
        (name, tail)
    };
    crate::router::open_dialog(
        app,
        crate::DialogKind::ForgetController,
        &name,
        &tail,
        &[crate::DialogButton::No, crate::DialogButton::Yes],
        0,
    );
}

pub(crate) fn forget_answered(ctx: &Ctx, app: &App, confirmed: bool) {
    let Some((id, _)) = lock(&ctx.shared).controllers.pending_forget.take() else {
        return;
    };
    if !confirmed {
        return;
    }
    request(
        ctx,
        app,
        move |device| async move { device.forget(&id).await },
        |ctx, app, result| {
            if let Err(err) = result {
                tracing::warn!("bluetooth forget failed: {}", err.message);
                crate::router::report_action_error(ctx, app, "bluetooth", "forget");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::device::{
        BluetoothAdapter, BluetoothPairing, BluetoothState, Capabilities, ControllerBattery,
    };

    fn pad(id: &str, name: &str, level: &str, charge: Option<&str>) -> Controller {
        Controller {
            id: id.into(),
            name: Some(name.into()),
            connection: "bluetooth".into(),
            battery: Some(ControllerBattery {
                percent: Some(10),
                level: level.into(),
                charge_state: charge.map(Into::into),
            }),
            ..Controller::default()
        }
    }

    fn device(id: &str, name: &str, paired: bool) -> BluetoothDevice {
        BluetoothDevice {
            id: id.into(),
            name: Some(name.into()),
            paired,
            connected: paired,
            gamepad: true,
            ..BluetoothDevice::default()
        }
    }

    fn model(controllers: Vec<Controller>, devices: Vec<BluetoothDevice>) -> Model {
        let mut capabilities = Capabilities::default();
        capabilities
            .actions
            .insert(ACTION_BLUETOOTH_PAIR.into(), "supported".into());
        Model {
            device: DeviceState {
                seeded: true,
                capabilities,
                power: None,
                bluetooth: Some(BluetoothAdapter {
                    present: true,
                    powered: Some(true),
                }),
                controllers: Some(controllers),
                bluetooth_devices: Some(BluetoothState {
                    powered: true,
                    scanning: true,
                    devices,
                    pairing: None,
                }),
            },
            ..Model::default()
        }
    }

    fn open(m: &mut Model) {
        m.session = Some(Session::default());
        m.observe();
        m.session
            .as_mut()
            .unwrap_or_else(|| unreachable!())
            .pair
            .search_answered(true);
        m.observe();
    }

    #[test]
    fn the_header_warns_about_a_flat_controller_that_is_not_charging() {
        assert!(!any_low(&[]));
        assert!(any_low(&[pad("a", "A", "low", Some("discharging"))]));
        assert!(!any_low(&[pad("a", "A", "low", Some("charging"))]));
        assert!(!any_low(&[Controller {
            battery: None,
            ..pad("usb", "U", "low", None)
        }]));
    }

    #[test]
    fn the_registry_sees_the_session_lists() {
        let mut m = model(
            vec![pad("in1", "Pad", "full", None)],
            vec![
                device("P", "Paired pad", true),
                device("F", "Found pad", false),
            ],
        );
        let mut inputs = zaparoo_app::settings::Inputs::default();
        m.fill_inputs(&mut inputs);
        assert!(inputs.controllers_listed && inputs.can_pair_controllers);
        assert_eq!((inputs.bluetooth_found, inputs.bluetooth_paired), (0, 0));
        open(&mut m);
        let mut inputs = zaparoo_app::settings::Inputs::default();
        m.fill_inputs(&mut inputs);
        assert_eq!((inputs.bluetooth_found, inputs.bluetooth_paired), (1, 1));
        let row = Row::Item {
            id: "foundDevice",
            index: 0,
            control: zaparoo_app::settings::Control::Device,
        };
        assert_eq!(m.row_key(row), "found:F");
        let mut out = crate::SettingsRow::default();
        m.fill_row(row, &mut out);
        assert_eq!(out.device, crate::DeviceValue::Found);
        assert_eq!(out.value_name, "Found pad");
        let paired = Row::Item {
            id: "pairedDevice",
            index: 0,
            control: zaparoo_app::settings::Control::Device,
        };
        let mut out = crate::SettingsRow::default();
        m.fill_row(paired, &mut out);
        assert_eq!(out.device, crate::DeviceValue::Connected);
        assert_eq!(m.row_key(paired), "paired:P");
    }

    #[test]
    fn a_find_core_drops_stays_and_a_new_one_goes_last() {
        let mut m = model(Vec::new(), vec![device("A", "A", false)]);
        open(&mut m);
        m.device.bluetooth_devices = Some(BluetoothState {
            powered: true,
            scanning: false,
            devices: vec![device("B", "B", false)],
            pairing: Some(BluetoothPairing::default()),
        });
        m.observe();
        let key = |m: &Model, i| {
            m.row_key(Row::Item {
                id: "foundDevice",
                index: i,
                control: zaparoo_app::settings::Control::Device,
            })
        };
        assert_eq!(key(&m, 0), "found:A");
        assert_eq!(key(&m, 1), "found:B");
    }

    #[test]
    fn paired_rows_ride_out_an_adapter_reset_and_a_forget_drops_one() {
        let mut m = model(
            Vec::new(),
            vec![device("A", "A", true), device("B", "B", true)],
        );
        open(&mut m);
        let paired = |m: &Model| m.session.as_ref().map(|s| s.paired.clone());
        // The dongle drops off the bus: Core reports it off with nothing on it.
        m.device.bluetooth_devices = Some(BluetoothState::default());
        m.observe();
        assert_eq!(paired(&m), Some(vec!["A".into(), "B".into()]));
        // Back again, BlueZ lists them in its own order; the page keeps its.
        m.device.bluetooth_devices = Some(BluetoothState {
            powered: true,
            devices: vec![device("B", "B", true), device("A", "A", true)],
            ..BluetoothState::default()
        });
        m.observe();
        assert_eq!(paired(&m), Some(vec!["A".into(), "B".into()]));
        m.device.bluetooth_devices = Some(BluetoothState {
            powered: true,
            devices: vec![device("B", "B", true)],
            ..BluetoothState::default()
        });
        m.observe();
        assert_eq!(paired(&m), Some(vec!["B".into()]));
    }

    #[test]
    fn same_names_on_the_page_carry_their_address_tails() {
        let mut first = pad("in1", "Wireless Controller", "full", None);
        first.display_name = None;
        let mut connected = device("A4:AE:12:34:79:CA", "Wireless Controller", true);
        connected.controller_id = Some("in1".into());
        let found = device("A4:AE:12:34:11:0F", "Wireless Controller", false);
        let mut m = model(vec![first], vec![connected, found]);
        open(&mut m);
        let slots = m.slots();
        assert!(slots[0].filled);
        assert_eq!(slots[0].tail, "79:CA");
        assert!(!slots[1].filled);
        let mut out = crate::SettingsRow::default();
        m.fill_row(
            Row::Item {
                id: "foundDevice",
                index: 0,
                control: zaparoo_app::settings::Control::Device,
            },
            &mut out,
        );
        assert_eq!(out.name_tail, "11:0F");
    }

    #[test]
    fn a_card_reads_its_charge() {
        let mut m = model(
            vec![
                pad("a", "A", "low", Some("charging")),
                Controller {
                    battery: None,
                    connection: "usb".into(),
                    ..pad("usb", "Wired pad", "full", None)
                },
                Controller {
                    battery: None,
                    ..pad("le", "Xbox Wireless Controller", "full", None)
                },
            ],
            Vec::new(),
        );
        open(&mut m);
        let slots = m.slots();
        assert_eq!(slots[0].reading, crate::DeviceValue::ChargingPercent);
        assert_eq!((slots[0].percent, slots[0].bar), (10, 10));
        assert!(slots[0].charging && !slots[0].low);
        assert_eq!(slots[1].reading, crate::DeviceValue::Wired);
        assert_eq!(slots[1].bar, -1);
        // Over Bluetooth with no battery reported: no reading, no bar.
        assert_eq!(slots[2].reading, crate::DeviceValue::None);
        assert_eq!(slots[2].bar, -1);
    }

    #[test]
    fn an_older_core_offers_no_row_and_no_pairing() {
        let m = Model::default();
        let mut inputs = zaparoo_app::settings::Inputs::default();
        m.fill_inputs(&mut inputs);
        assert!(!inputs.controllers_listed && !inputs.can_pair_controllers);
        let mut off = model(Vec::new(), Vec::new());
        off.device.bluetooth = Some(BluetoothAdapter {
            present: true,
            powered: Some(false),
        });
        assert!(off.bluetooth_off());
        assert_eq!(off.search(), crate::PairSearch::AdapterOff);
    }
}
