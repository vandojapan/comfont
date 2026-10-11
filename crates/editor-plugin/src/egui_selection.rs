// Selection is independent of the model's active row: Ctrl may leave no rows selected.
#[derive(Default, Clone, Debug)]
pub(super) struct RowSelection {
    pub rows: Vec<usize>,
    anchor: Option<usize>,
}
impl RowSelection {
    pub fn single(&mut self, row: usize, count: usize) {
        self.rows = if row < count { vec![row] } else { vec![] };
        self.anchor = self.rows.first().copied();
    }
    pub fn replace(&mut self, rows: Vec<usize>) {
        self.rows = rows;
        self.anchor = self.rows.first().copied();
    }
    pub fn click(&mut self, row: usize, count: usize, shift: bool, ctrl: bool) {
        if row >= count {
            return;
        }
        if shift {
            let anchor = self.anchor.unwrap_or(row).min(count - 1);
            if !ctrl {
                self.rows.clear();
            }
            self.rows.extend(anchor.min(row)..=anchor.max(row));
            self.anchor = Some(anchor);
        } else if ctrl {
            if self.rows.contains(&row) {
                self.rows.retain(|&r| r != row);
            } else {
                self.rows.push(row);
            }
            self.anchor = Some(row);
        } else {
            self.single(row, count);
        }
        self.rows.sort_unstable();
        self.rows.dedup();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_range_from_anchor_and_plain_click_clears_it() {
        let mut selection = RowSelection::default();
        selection.single(4, 7);
        selection.click(1, 7, true, false);
        assert_eq!(selection.rows, vec![1, 2, 3, 4]);
        selection.click(6, 7, true, false);
        assert_eq!(selection.rows, vec![4, 5, 6]);
        selection.click(5, 7, false, false);
        assert_eq!(selection.rows, vec![5]);
    }
    #[test]
    fn ctrl_adds_and_removes_including_last_and_shift_ctrl_unions() {
        let mut selection = RowSelection::default();
        selection.single(0, 7);
        selection.click(3, 7, false, true);
        selection.click(5, 7, true, true);
        assert_eq!(selection.rows, vec![0, 3, 4, 5]);
        selection.click(0, 7, false, false);
        selection.click(0, 7, false, true);
        assert!(selection.rows.is_empty());
        selection.click(99, 7, true, false);
        assert!(selection.rows.is_empty());
    }
}
