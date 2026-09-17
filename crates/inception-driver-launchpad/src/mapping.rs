//! MIDI note ↔ `pad(x, y)` conversion for the Launchpad X's main 8x8
//! grid — the *only* place in this crate (or anywhere above it) allowed
//! to know a raw MIDI note number.
//!
//! ## Coordinate convention
//!
//! `x` is the column, `1..=8` left to right. `y` is the row, `1..=8`,
//! with `y = 1` the *top* row — matching how a lighting operator reads a
//! grid on a physical controller. The Launchpad X's own Session-layout
//! MIDI numbering is the classic Launchpad convention: note `11` is the
//! bottom-left pad, note `88` the top-right, incrementing by `1` per
//! column and by `10` per hardware row, with notes ending in `9`/`0`
//! (`19`, `20`, `29`, `30`, ...) unused. `pad_to_note`/`note_to_pad`
//! convert between our top-down `y` and that bottom-up hardware
//! numbering.

/// `(x, y)` -> the Launchpad X Session-layout MIDI note, or `None` if
/// either coordinate is outside `1..=8`.
pub fn pad_to_note(x: u8, y: u8) -> Option<u8> {
    if !(1..=8).contains(&x) || !(1..=8).contains(&y) {
        return None;
    }
    let hardware_row = 9 - y; // y = 1 (top) -> hardware row 8; y = 8 (bottom) -> hardware row 1.
    Some(10 * hardware_row + x)
}

/// The inverse of [`pad_to_note`]. `None` for any note outside the main
/// grid — including the unused "9"/"0"-ending notes between hardware
/// rows, which never correspond to a pad.
pub fn note_to_pad(note: u8) -> Option<(u8, u8)> {
    if !(11..=88).contains(&note) {
        return None;
    }
    let hardware_row = note / 10;
    let x = note % 10;
    if !(1..=8).contains(&hardware_row) || !(1..=8).contains(&x) {
        return None;
    }
    let y = 9 - hardware_row;
    Some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pad_on_the_main_grid_round_trips() {
        for x in 1..=8 {
            for y in 1..=8 {
                let note = pad_to_note(x, y).unwrap_or_else(|| panic!("pad ({x}, {y})"));
                assert_eq!(note_to_pad(note), Some((x, y)), "note {note}");
            }
        }
    }

    #[test]
    fn corners_match_the_documented_launchpad_note_numbers() {
        assert_eq!(pad_to_note(1, 8), Some(11)); // bottom-left
        assert_eq!(pad_to_note(8, 8), Some(18)); // bottom-right
        assert_eq!(pad_to_note(1, 1), Some(81)); // top-left
        assert_eq!(pad_to_note(8, 1), Some(88)); // top-right
    }

    #[test]
    fn out_of_range_coordinates_are_rejected() {
        assert_eq!(pad_to_note(0, 1), None);
        assert_eq!(pad_to_note(9, 2), None);
        assert_eq!(pad_to_note(1, 0), None);
        assert_eq!(pad_to_note(1, 9), None);
    }

    #[test]
    fn notes_outside_the_main_grid_are_rejected() {
        assert_eq!(note_to_pad(0), None);
        assert_eq!(note_to_pad(10), None);
        assert_eq!(note_to_pad(89), None);
        assert_eq!(note_to_pad(255), None);
    }

    #[test]
    fn gap_notes_between_hardware_rows_are_rejected() {
        for gap in [19, 20, 29, 30, 39, 40, 49, 50, 59, 60, 69, 70, 79, 80] {
            assert_eq!(note_to_pad(gap), None, "note {gap}");
        }
    }
}
