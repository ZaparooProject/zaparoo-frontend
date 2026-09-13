//! Avoid resetting delegates when a render republishes unchanged rows.

use slint::{Model, ModelRc, VecModel};

pub fn publish<T: Clone + PartialEq + 'static>(
    current: &ModelRc<T>,
    rows: Vec<T>,
    set: impl FnOnce(ModelRc<T>),
) {
    publish_by(current, rows, PartialEq::eq, set);
}

pub fn publish_cells(
    current: &ModelRc<crate::GridCell>,
    rows: Vec<crate::GridCell>,
    set: impl FnOnce(ModelRc<crate::GridCell>),
) {
    publish_keyed(current, rows, same_cell, same_slot, set);
}

/// Whether two cells are the same slot of the same page, so a cell whose
/// art finished loading updates in place instead of resetting the grid
/// and cancelling the focus zoom.
fn same_slot(left: &crate::GridCell, right: &crate::GridCell) -> bool {
    left.label_key == right.label_key && left.name == right.name && left.is_empty == right.is_empty
}

pub fn same_image(left: &slint::Image, right: &slint::Image) -> bool {
    left == right || (left.size().width == 0 && right.size().width == 0)
}

pub fn same_cell(left: &crate::GridCell, right: &crate::GridCell) -> bool {
    if !same_image(&left.cover, &right.cover) || !same_image(&left.cover_focus, &right.cover_focus)
    {
        return false;
    }
    // Image(None) is not equal even to its own clone, so derived cell
    // equality cannot describe content equality for placeholder tiles.
    left.label_key == right.label_key
        && left.name == right.name
        && left.glyph_key == right.glyph_key
        && left.has_cover == right.has_cover
        && left.has_cover_focus == right.has_cover_focus
        && left.hidden == right.hidden
        && left.disabled == right.disabled
        && left.favorite == right.favorite
        && left.tags == right.tags
        && left.top_label == right.top_label
        && left.wordmark == right.wordmark
        && left.is_empty == right.is_empty
}

fn publish_by<T: Clone + 'static>(
    current: &ModelRc<T>,
    rows: Vec<T>,
    same: impl Fn(&T, &T) -> bool,
    set: impl FnOnce(ModelRc<T>),
) {
    if current.row_count() == rows.len()
        && rows.iter().enumerate().all(|(i, row)| {
            current
                .row_data(i)
                .as_ref()
                .is_some_and(|old| same(old, row))
        })
    {
        // No new model means no delegate reset and no lost press state.
        return;
    }
    set(ModelRc::new(VecModel::from(rows)));
}

/// Republish `rows`, reusing the delegates where the list is still the
/// same list.
///
/// `same` asks whether a row's content is unchanged; `keyed` asks whether
/// two rows are the *same row*, which is a weaker question. When every
/// index still holds the same row, only the ones whose content moved are
/// written, in place, and Slint updates those delegates rather than
/// rebuilding the repeater.
///
/// That distinction is load-bearing. A replaced model resets every
/// delegate, and a freshly created element starts at its target value
/// with nothing to animate from, so a single changed row silently kills
/// every animation in the list: a toggle knob sliding, a tile's focus
/// zoom, a press cue mid-flight. Replacing is still right when the list
/// becomes a *different* list, because animating a knob between two
/// unrelated rows would be nonsense; that is what `keyed` protects.
pub fn publish_keyed<T: Clone + 'static>(
    current: &ModelRc<T>,
    rows: Vec<T>,
    same: impl Fn(&T, &T) -> bool,
    keyed: impl Fn(&T, &T) -> bool,
    set: impl FnOnce(ModelRc<T>),
) {
    if current.row_count() != rows.len() {
        set(ModelRc::new(VecModel::from(rows)));
        return;
    }
    let mut stale = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let Some(old) = current.row_data(i) else {
            set(ModelRc::new(VecModel::from(rows)));
            return;
        };
        if !keyed(&old, row) {
            set(ModelRc::new(VecModel::from(rows)));
            return;
        }
        if !same(&old, row) {
            stale.push(i);
        }
    }
    // No new model means no delegate reset and no lost press state.
    for i in stale {
        current.set_row_data(i, rows[i].clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_images_are_content_equal_but_focus_art_state_is_not() {
        let left = crate::GridCell::default();
        let mut right = crate::GridCell::default();
        assert!(same_cell(&left, &right));
        right.has_cover_focus = true;
        assert!(!same_cell(&left, &right));
    }

    #[test]
    fn identical_rows_skip_publication_but_changed_rows_publish() {
        let original = ModelRc::new(VecModel::from(vec![1, 2]));
        let mut published = false;
        publish(&original, vec![1, 2], |_| published = true);
        assert!(!published, "unchanged rows must not reset delegates");
        let mut changed = None;
        publish(&original, vec![1, 3], |rows| changed = Some(rows));
        assert_eq!(changed.and_then(|rows| rows.row_data(1)), Some(3));
        publish(&original, vec![], |rows| assert_eq!(rows.row_count(), 0));
    }

    #[test]
    fn a_changed_row_updates_in_place_instead_of_resetting_the_list() {
        // The delegate reset is what kills in-row animation, so a list
        // that is still the same list must never be replaced.
        let current = ModelRc::new(VecModel::from(vec![1, 2, 3]));
        let mut replaced = false;
        publish_keyed(
            &current,
            vec![1, 9, 3],
            PartialEq::eq,
            |_, _| true,
            |_| replaced = true,
        );
        assert!(!replaced, "same list must not reset delegates");
        assert_eq!(current.row_data(1), Some(9), "changed row written in place");
        assert_eq!(current.row_data(0), Some(1));
    }

    #[test]
    fn a_different_list_still_replaces_the_model() {
        let current = ModelRc::new(VecModel::from(vec![1, 2]));
        let mut replaced = None;
        publish_keyed(
            &current,
            vec![7, 8],
            PartialEq::eq,
            |_, _| false,
            |rows| replaced = Some(rows),
        );
        assert_eq!(
            replaced.and_then(|rows| rows.row_data(0)),
            Some(7),
            "a list of different rows resets, so nothing animates across them"
        );
        // A length change is always a different list.
        let mut resized = None;
        publish_keyed(
            &current,
            vec![1],
            PartialEq::eq,
            |_, _| true,
            |rows| {
                resized = Some(rows);
            },
        );
        assert_eq!(resized.map(|rows| rows.row_count()), Some(1));
    }

    #[test]
    fn a_cell_keeps_its_slot_while_its_art_loads() {
        let mut placeholder = crate::GridCell {
            name: "Sonic".into(),
            ..Default::default()
        };
        let mut loaded = placeholder.clone();
        loaded.has_cover = true;
        assert!(same_slot(&placeholder, &loaded), "same tile, new art");
        assert!(!same_cell(&placeholder, &loaded), "content did move");
        placeholder.name = "Mario".into();
        assert!(!same_slot(&placeholder, &loaded), "a different tile resets");
    }
}
