pub struct Ranges {
    r: Vec<(u64, u64)>,
}

/// Returns the lowest i such that f(v[i]) > x,
// or v.len() if there is no such i.
/// v needs to be sorted in ascending order,
///   f(v[i]) < f(v[j]) for all i < j
fn bisection<S, T, F>(v: &[S], f: F, x: T) -> usize
    where F: Fn(&S) -> T,
          T: Ord
{
    match v.binary_search_by_key(&x, f) {
        Ok(index) => index + 1,
        Err(index) => index,
    }
}

impl Ranges {
    pub fn new() -> Ranges {
        Ranges { r: vec![] }
    }

    pub fn get(&self) -> Vec<(u64, u64)> {
        self.r.clone()
    }

    pub fn add(&mut self, mut start: u64, size: u64) {
        let mut end = start + size;
        let insertion_index_start_start = bisection(&self.r, |&(s, _)| s, start);
        let insertion_index_start_end = bisection(&self.r, |&(_, e)| e, start);
        let insertion_index_end_start = bisection(&self.r, |&(s, _)| s, end);
        let insertion_index_end_end = bisection(&self.r, |&(_, e)| e, end);
        let mut first_removal_index = insertion_index_start_end;
        let mut after_last_removal_index = insertion_index_end_end;
        if insertion_index_start_start != insertion_index_start_end {
            assert!(insertion_index_start_start > insertion_index_start_end);
            // start falls into the range at insertion_index_start_end
            start = self.r[insertion_index_start_end].0;
            first_removal_index = insertion_index_start_end;
        } else {
            // start is before the range at insertion_index_start_start
        }
        if insertion_index_end_start != insertion_index_end_end {
            assert!(insertion_index_end_start > insertion_index_end_end);
            // end falls into the range at insertion_index_end_end
            end = self.r[insertion_index_end_end].1;
            after_last_removal_index = insertion_index_end_start;
        } else {
            // end is before the range at insertion_index_end_start
        }
        if first_removal_index != 0 && self.r[first_removal_index - 1].1 == start {
            start = self.r[first_removal_index - 1].0;
            first_removal_index = first_removal_index - 1;
        }
        if after_last_removal_index != 0 && after_last_removal_index < self.r.len() &&
           self.r[after_last_removal_index - 1].0 == end {
            end = self.r[after_last_removal_index - 1].1;
            after_last_removal_index = after_last_removal_index + 1;
        }
        for i in (first_removal_index..after_last_removal_index).rev() {
            self.r.remove(i);
        }
        self.r.insert(first_removal_index, (start, end));
        self.assert_consistency();
    }

    pub fn cumulative_size(&self) -> u64 {
        self.r.iter().fold(0, |sum, &(s, e)| sum + (e - s))
    }

    fn assert_consistency(&self) {
        if !self.r.is_empty() {
            let (first_start, first_end) = self.r[0];
            if !(first_start < first_end) {
                panic!("first range is empty or upside down, {}, {}",
                       first_start,
                       first_end);
            }
            let mut prev_end = first_end;
            for &(start, end) in self.r.iter().skip(1) {
                if !(prev_end < start) {
                    panic!("start is not strictly larger than prev_end! {}, {}",
                           prev_end,
                           start);
                }
                if !(start < end) {
                    panic!("end is not strictly larger than start! {}, {}", start, end);
                }
                prev_end = end;
            }
        }
        for &(ref start, ref end) in &self.r {
            if start >= end {
                panic!("upside down: {} >= {}", start, end);
            }
        }
    }

    pub fn remove(&mut self, start: u64, size: u64) {
        // console.log(this._startAddresses.slice(), this._endAddresses.slice());
        let end = start + size;
        // console.log('removing', start, end);
        let insertion_index_start_start = bisection(&self.r, |&(s, _)| s, start);
        let insertion_index_start_end = bisection(&self.r, |&(_, e)| e, start);
        let insertion_index_end_start = bisection(&self.r, |&(s, _)| s, end);
        let insertion_index_end_end = bisection(&self.r, |&(_, e)| e, end);
        let first_removal_index = insertion_index_start_end;
        let after_last_removal_index = insertion_index_end_start;
        let mut new_first_range_start = None;
        let mut new_second_range_end = None;
        if insertion_index_start_start != insertion_index_start_end {
            assert!(insertion_index_start_start > insertion_index_start_end);
            // start falls into the range at insertion_index_start_end
            let new_first_range_start_candidate = self.r[insertion_index_start_end].0;
            if new_first_range_start_candidate != start {
                new_first_range_start = Some(new_first_range_start_candidate);
            }
        } else {
            // start is before the range at insertion_index_start_start
        }
        if insertion_index_end_start != insertion_index_end_end {
            assert!(insertion_index_end_start > insertion_index_end_end);
            // end falls into the range at insertion_index_end_end
            let new_second_range_end_candidate = self.r[insertion_index_end_end].1;
            if new_second_range_end_candidate != end {
                new_second_range_end = Some(new_second_range_end_candidate);
            }
        } else {
            // end is before the range at insertion_index_end_start
        }
        for i in (first_removal_index..after_last_removal_index).rev() {
            self.r.remove(i);
        }
        if let Some(new_second_range_end) = new_second_range_end {
            self.r
                .insert(first_removal_index, (end, new_second_range_end));
        }
        if let Some(new_first_range_start) = new_first_range_start {
            self.r
                .insert(first_removal_index, (new_first_range_start, start));
        }
        self.assert_consistency();
    }

    pub fn contains(&self, value: u64) -> bool {
        let range_index = bisection(&self.r, |&(_, e)| e, value);
        if range_index >= self.r.len() {
            return false;
        }
        let (start, end) = self.r[range_index];
        start <= value && value < end
    }
}

#[test]
fn test_bisect() {
    assert_eq!(bisection(&[0, 10, 20], |x| *x, 5), 1);
    assert_eq!(bisection(&[0, 10, 20], |x| *x, 10), 2);
    assert_eq!(bisection(&[0, 10, 20], |x| *x, 0), 1);
    assert_eq!(bisection(&[0, 10, 20], |x| *x, -5), 0);
}

#[test]
fn test_ranges() {
    let mut ranges = Ranges::new();
    ranges.add(10, 10);
    ranges.add(20, 10);
    assert_eq!(ranges.get(), [(10, 30)]);

    let mut ranges = Ranges::new();
    ranges.add(10, 10);
    ranges.add(30, 10);
    ranges.add(20, 10);
    assert_eq!(ranges.get(), [(10, 40)]);

    let mut ranges = Ranges::new();
    ranges.add(30, 10);
    ranges.add(10, 10);
    ranges.add(20, 10);
    assert_eq!(ranges.get(), [(10, 40)]);

    let mut ranges = Ranges::new();
    ranges.add(30, 10);
    ranges.add(10, 10);
    ranges.add(15, 20);
    assert_eq!(ranges.get(), [(10, 40)]);

    let mut ranges = Ranges::new();
    ranges.add(30, 10);
    ranges.add(10, 10);
    ranges.add(15, 20);
    assert_eq!(ranges.get(), [(10, 40)]);

    let mut ranges = Ranges::new();
    ranges.add(10, 30);
    assert_eq!(ranges.get(), [(10, 40)]);
    ranges.remove(20, 10);
    assert_eq!(ranges.get(), [(10, 20), (30, 40)]);
    ranges.add(20, 10);
    assert_eq!(ranges.get(), [(10, 40)]);
    ranges.add(50, 10);
    assert_eq!(ranges.get(), [(10, 40), (50, 60)]);
    ranges.remove(0, 15);
    assert_eq!(ranges.get(), [(15, 40), (50, 60)]);
    ranges.remove(55, 2);
    assert_eq!(ranges.get(), [(15, 40), (50, 55), (57, 60)]);
    ranges.remove(53, 2);
    assert_eq!(ranges.get(), [(15, 40), (50, 53), (57, 60)]);
    ranges.add(50, 5);
    assert_eq!(ranges.get(), [(15, 40), (50, 55), (57, 60)]);
    ranges.remove(56, 20);
    assert_eq!(ranges.get(), [(15, 40), (50, 55)]);
    ranges.add(55, 5);
    assert_eq!(ranges.get(), [(15, 40), (50, 60)]);
    ranges.remove(40, 10);
    assert_eq!(ranges.get(), [(15, 40), (50, 60)]);
    ranges.remove(39, 10);
    assert_eq!(ranges.get(), [(15, 39), (50, 60)]);
    ranges.remove(38, 1);
    assert_eq!(ranges.get(), [(15, 38), (50, 60)]);
    ranges.remove(0, 17);
    assert_eq!(ranges.get(), [(17, 38), (50, 60)]);
    ranges.remove(18, 40);
    assert_eq!(ranges.get(), [(17, 18), (58, 60)]);
    ranges.add(19, 5);
    assert_eq!(ranges.get(), [(17, 18), (19, 24), (58, 60)]);
    ranges.add(27, 5);
    assert_eq!(ranges.get(), [(17, 18), (19, 24), (27, 32), (58, 60)]);
    ranges.add(38, 10);
    assert_eq!(ranges.get(), [(17, 18), (19, 24), (27, 32), (38, 48), (58, 60)]);
    ranges.remove(18, 41);
    assert_eq!(ranges.get(), [(17, 18), (59, 60)]);
}
