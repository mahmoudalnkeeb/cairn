//! Which tiles in the stack are selected (C4): click, Ctrl click and Shift
//! click, as in Explorer. Selection is kept by item id so it survives items
//! being added in front.

#[derive(Default)]
pub struct Selection {
    ids: Vec<u64>,
    /// Where a Shift range starts.
    anchor: usize,
}

impl Selection {
    pub fn contains(&self, id: u64) -> bool {
        self.ids.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Plain click or arrow key: just this one.
    pub fn only(&mut self, ids: &[u64], i: usize) {
        self.ids = ids.get(i).copied().into_iter().collect();
        self.anchor = i;
    }

    /// Ctrl click: add or drop one, keeping the rest.
    pub fn toggle(&mut self, ids: &[u64], i: usize) {
        let Some(&id) = ids.get(i) else { return };
        match self.ids.iter().position(|&s| s == id) {
            Some(at) => {
                self.ids.remove(at);
            }
            None => self.ids.push(id),
        }
        self.anchor = i;
    }

    /// Shift click or Shift arrow: everything from the anchor to here.
    pub fn extend(&mut self, ids: &[u64], i: usize) {
        if ids.is_empty() {
            return;
        }
        let i = i.min(ids.len() - 1);
        let a = self.anchor.min(ids.len() - 1);
        self.ids = ids[a.min(i)..=a.max(i)].to_vec();
    }

    pub fn all(&mut self, ids: &[u64]) {
        self.ids = ids.to_vec();
        self.anchor = 0;
    }

    /// Forgets items that are no longer in the stack.
    pub fn retain(&mut self, ids: &[u64]) {
        self.ids.retain(|s| ids.contains(s));
        self.anchor = self.anchor.min(ids.len().saturating_sub(1));
    }

    /// Selected ids in stack order.
    pub fn ordered(&self, ids: &[u64]) -> Vec<u64> {
        ids.iter().copied().filter(|&id| self.contains(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: [u64; 5] = [10, 11, 12, 13, 14];

    #[test]
    fn click_selects_one() {
        let mut s = Selection::default();
        s.all(&IDS);
        s.only(&IDS, 2);
        assert_eq!(s.ordered(&IDS), [12]);
    }

    #[test]
    fn ctrl_click_toggles() {
        let mut s = Selection::default();
        s.only(&IDS, 0);
        s.toggle(&IDS, 3);
        assert_eq!(s.ordered(&IDS), [10, 13]);
        s.toggle(&IDS, 0);
        assert_eq!(s.ordered(&IDS), [13]);
    }

    #[test]
    fn shift_click_takes_a_range_from_the_anchor() {
        let mut s = Selection::default();
        s.only(&IDS, 3);
        s.extend(&IDS, 1);
        assert_eq!(s.ordered(&IDS), [11, 12, 13]);
        // A second Shift click moves the far end, not the anchor.
        s.extend(&IDS, 4);
        assert_eq!(s.ordered(&IDS), [13, 14]);
    }

    #[test]
    fn ctrl_click_moves_the_anchor() {
        let mut s = Selection::default();
        s.only(&IDS, 0);
        s.toggle(&IDS, 2);
        s.extend(&IDS, 4);
        assert_eq!(s.ordered(&IDS), [12, 13, 14]);
    }

    #[test]
    fn survives_removal_and_new_items() {
        let mut s = Selection::default();
        s.all(&IDS);
        let after = [20, 10, 12, 14];
        s.retain(&after);
        assert_eq!(s.len(), 3);
        assert_eq!(s.ordered(&after), [10, 12, 14]);
        assert!(!s.contains(20));
    }
}
