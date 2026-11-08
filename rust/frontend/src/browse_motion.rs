//! Minimal scrolling for Slint's bounded list window. Selection remains
//! stationary until it reaches a viewport edge, rather than staying
//! centered while the list moves under it.

pub fn window_top(current: usize, count: usize, visible: usize, previous: usize) -> usize {
    let visible = visible.max(1);
    previous
        .min(current)
        .max(current.saturating_sub(visible - 1))
        .min(count.saturating_sub(visible))
}

/// A browse screen's fast scroll and its rail: the rail comes up with the
/// scroll and stays `RAIL_LINGER_MS` after it ends.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rapid {
    pub active: bool,
    pub rail_visible: bool,
    rail_seq: u64,
}

impl Rapid {
    /// The scroll started or stopped. Returns whether it changed, and the
    /// ticket of a linger to arm. Only the change re-arms the linger: taps
    /// after a fast scroll must not keep pushing its deadline back.
    pub fn set(&mut self, active: bool) -> (bool, Option<u64>) {
        let changed = self.active != active;
        self.active = active;
        if changed {
            self.rail_seq += 1;
        }
        let linger = changed && !active && self.rail_visible;
        if active {
            self.rail_visible = true;
        }
        (changed, linger.then_some(self.rail_seq))
    }

    /// The linger with ticket `seq` ran out: true when it still owns the
    /// rail, which is now down.
    pub fn linger_ended(&mut self, seq: u64) -> bool {
        if self.rail_seq != seq {
            return false;
        }
        self.rail_visible = false;
        true
    }
}

/// Take the rail down once its linger runs out, unless a newer scroll owns
/// it. `rapid` finds the screen's state and `hide` clears the view's rail.
pub(crate) fn linger_rail(
    ctx: &crate::router::Ctx,
    app: &crate::App,
    seq: u64,
    rapid: fn(&mut crate::router::Shared) -> &mut Rapid,
    hide: fn(&crate::App),
) {
    use slint::ComponentHandle;
    let weak = app.as_weak();
    let ctx = ctx.clone();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(zaparoo_app::media_list::RAIL_LINGER_MS),
        move || {
            if !rapid(&mut crate::router::lock(&ctx.shared)).linger_ended(seq) {
                return;
            }
            if let Some(app) = weak.upgrade() {
                hide(&app);
            }
        },
    );
}

/// The list properties every browse view global carries.
pub(crate) trait ListView {
    fn list_rows(&self) -> slint::ModelRc<crate::GridCell>;
    fn put_list_rows(&self, rows: slint::ModelRc<crate::GridCell>);
    /// The row the last render selected, in model order.
    fn list_selected(&self) -> usize;
    fn put_list_follow(&self, glide: bool, step_ms: i32);
    fn put_list_window(&self, sel: usize, view_top: usize, scroll_top: usize);
}

macro_rules! list_view {
    ($($view:ident),+ $(,)?) => {
        $(
            impl ListView for crate::$view<'_> {
                fn list_rows(&self) -> slint::ModelRc<crate::GridCell> {
                    self.get_list_rows()
                }
                fn put_list_rows(&self, rows: slint::ModelRc<crate::GridCell>) {
                    self.set_list_rows(rows);
                }
                fn list_selected(&self) -> usize {
                    usize::try_from(self.get_list_view_top() + self.get_list_sel()).unwrap_or(0)
                }
                fn put_list_follow(&self, glide: bool, step_ms: i32) {
                    self.set_list_glide(glide);
                    self.set_list_step_ms(step_ms);
                }
                fn put_list_window(&self, sel: usize, view_top: usize, scroll_top: usize) {
                    self.set_list_sel(i32::try_from(sel).unwrap_or(0));
                    self.set_list_view_top(i32::try_from(view_top).unwrap_or(0));
                    self.set_list_scroll_top(i32::try_from(scroll_top).unwrap_or(0));
                }
            }
        )+
    };
}

list_view!(GamesView, SystemsView, FileBrowserView, MusicView);

/// Publish a browse list's window around `current`: the rows on screen
/// plus a page either side, so a glide to the next row or the next page
/// has rows to show the whole way, and how the list follows this move.
/// Only a render that moved the selection decides that; one that did not
/// leaves a glide in flight as it is. Returns the window's top row.
#[allow(
    clippy::too_many_arguments,
    reason = "one call publishes the whole window for any of the browse lists"
)]
pub(crate) fn publish_list<R>(
    view: &impl ListView,
    input: &crate::input::InputModel,
    current: usize,
    count: usize,
    visible: usize,
    previous: usize,
    rows: &[R],
    cell: impl Fn(&R) -> crate::GridCell,
) -> usize {
    publish_list_window(
        view,
        input,
        current,
        count,
        visible,
        previous,
        |top, take| {
            let cells = rows.iter().skip(top).take(take).map(cell).collect();
            (top, cells)
        },
    )
}

/// The same for rows that are not one slice: `window` gets the wanted top
/// and row count and answers with the top it could start from and its cells.
pub(crate) fn publish_list_window(
    view: &impl ListView,
    input: &crate::input::InputModel,
    current: usize,
    count: usize,
    visible: usize,
    previous: usize,
    window: impl FnOnce(usize, usize) -> (usize, Vec<crate::GridCell>),
) -> usize {
    let scroll_top = window_top(current, count, visible, previous);
    let wanted_top = scroll_top.saturating_sub(visible);
    let (top, cells) = window(wanted_top, scroll_top - wanted_top + 2 * visible);
    let moved = current.abs_diff(view.list_selected());
    if moved > 0 {
        let (glide, step_ms) = input.list_follow(moved);
        view.put_list_follow(glide, step_ms);
    }
    crate::view_model::publish_cells(&view.list_rows(), cells, |rows| view.put_list_rows(rows));
    view.put_list_window(current.saturating_sub(top), top, scroll_top);
    scroll_top
}

#[cfg(test)]
mod tests {
    use super::{window_top, Rapid};

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "the scroll that showed the rail arms a linger"
    )]
    fn the_rail_lingers_only_after_a_scroll_that_showed_it() {
        let mut rapid = Rapid::default();
        assert_eq!(rapid.set(false), (false, None));
        assert_eq!(rapid.set(true), (true, None));
        assert!(rapid.rail_visible);
        let (changed, linger) = rapid.set(false);
        assert!(changed);
        let seq = linger.expect("a linger");
        assert_eq!(rapid.set(false), (false, None), "a tap does not re-arm it");
        assert!(rapid.linger_ended(seq));
        assert!(!rapid.rail_visible);
        assert_eq!(rapid.set(true).1, None);
        assert!(!rapid.linger_ended(seq), "a newer scroll owns the rail");
    }

    #[test]
    fn focus_moves_without_recentering_until_it_reaches_an_edge() {
        assert_eq!(window_top(4, 100, 8, 0), 0);
        assert_eq!(window_top(7, 100, 8, 0), 0);
        assert_eq!(window_top(8, 100, 8, 0), 1);
        assert_eq!(window_top(7, 100, 8, 1), 1);
        assert_eq!(window_top(0, 100, 8, 1), 0);
    }

    #[test]
    fn jumps_shrinkage_and_empty_models_clamp_the_window() {
        assert_eq!(window_top(99, 100, 8, 1), 92);
        assert_eq!(window_top(0, 0, 0, 92), 0);
        assert_eq!(window_top(2, 3, 8, 92), 0);
        assert_eq!(window_top(2, 100, 1, 0), 2);
    }
}
