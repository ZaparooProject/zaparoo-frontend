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
    publish_by(current, rows, same_cell, set);
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
}
