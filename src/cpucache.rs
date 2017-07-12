use ranges::Ranges;

pub struct CPUCache {
    line_size: u8,
    line_size_bits: u8,
    sets_min_1: u64,
    assoc: u64,
    tags: Vec<u64>,
}

/* Returns the base-2 logarithm of x.  Returns None if x is not a power
   of two. */
fn log2(x: u32) -> Option<u8> {
    /* Any more than 32 and we overflow anyway... */
    for i in 0..32 {
        if (1u32 << i) == x {
            return Some(i);
        }
    }
    None
}

impl CPUCache {
    pub fn new(size: u32, line_size: u8, assoc: u32) -> CPUCache {
        let sets = ((size / line_size as u32) / assoc) as u64;
        CPUCache {
            // size: size as u64,
            line_size: line_size,
            line_size_bits: log2(line_size as u32).unwrap(),
            // sets,
            sets_min_1: sets - 1,
            assoc: assoc as u64,
            tags: vec![0; (size / line_size as u32) as usize],
        }
    }

    pub fn load(&mut self, addr: u64) -> Option<u64> {
        let tag = addr >> self.line_size_bits;
        let set_no = tag & self.sets_min_1;
        let tag_index_start = (set_no * self.assoc) as usize;
        let tag_index_end = ((set_no + 1) * self.assoc) as usize;
        let set = &mut self.tags[tag_index_start..tag_index_end];
        if tag == set[0] {
            return None;
        }
        for i in 1..(self.assoc as usize) {
            if tag == set[i] {
                let mut j = i;
                while j > 0 {
                    set[j] = set[j - 1];
                    j = j - 1;
                }
                set[0] = tag;

                return None;
            }
        }

        let mut j = (self.assoc - 1) as usize;
        let evicting = set[j] << self.line_size_bits;
        while j > 0 {
            set[j] = set[j - 1];
            j = j - 1;
        }
        set[0] = tag;

        Some(evicting)
    }

    pub fn exchange(&mut self, new_addr: u64, old_addr: u64) {
        let old_tag = old_addr >> self.line_size_bits;
        let new_tag = new_addr >> self.line_size_bits;
        let old_set_no = old_tag & self.sets_min_1;
        let new_set_no = new_tag & self.sets_min_1;
        if old_tag != 0 && old_set_no != new_set_no {
            panic!(
                "Expected to only exchange cache lines inside the same set! old_addr={:x} new_addr={:x} old_set_no={} new_set_no={}",
                old_addr,
                new_addr,
                old_set_no,
                new_set_no
            );
        }
        let set_no = new_set_no;
        let tag_index_start = (set_no * self.assoc) as usize;
        let tag_index_end = ((set_no + 1) * self.assoc) as usize;
        let set = &mut self.tags[tag_index_start..tag_index_end];
        for tag in set.iter_mut() {
            if *tag == old_tag {
                *tag = new_tag;
                return;
            }
        }
        panic!("Couldn't find tag {:x} in set {}", old_tag, set_no);
    }

    pub fn get_cached_ranges(&self) -> Vec<(u64, u64)> {
        let mut ranges = Ranges::new();
        for tag in &self.tags {
            if *tag != 0 {
                let start = tag << self.line_size_bits;
                ranges.add(start, self.line_size as u64);
            }
        }
        ranges.get()
    }
}
