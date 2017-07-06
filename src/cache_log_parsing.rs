use std::io::{self, BufRead};
use std::num::ParseIntError;
use std::iter;
use std::result;
use nom::{hex_digit, digit, rest_s, IResult};
use std::str::FromStr;
use std::collections::HashMap;
use std::fmt::Display;

use shared_libraries::SharedLibraries;

#[derive(Debug,PartialEq)]
pub enum LineContent<'a> {
    LLCacheInfo {
        size: u32,
        line_size: u32,
        assoc: u32,
    },
    LLCacheLineSwap {
        new_start: u64,
        old_start: u64,
        size: u64,
    },
    LLMiss {
        why: &'a str,
        size: u64,
        addr: u64,
        tid: u32,
    },
    StackForLLMiss(usize),
    BeginDisplayList,
    EndDisplayList,
    AddFrame { index: usize, address: u64 },
    AddStack {
        index: usize,
        parent_stack: usize,
        frame: usize,
    },
    AllocatingArenaChunk {
        ident: &'a str,
        chunk_start: u64,
        chunk_size: u64,
    },
    DeallocatingArenaChunk {
        ident: &'a str,
        chunk_start: u64,
        chunk_size: u64,
    },
    Association { ident1: &'a str, ident2: &'a str },
    ExtraField {
        ident: &'a str,
        field_name: &'a str,
        field_content: &'a str,
    },
    SharedLibsChunk(&'a str),
    Other(&'a str),
}

// LL cache information: 8388608 B, 64 B, 16-way associative
// LLMiss: caching 64 bytes at 0000000005cb2400, evicting 64 bytes at 0000000057eb2400
// LLMiss: why=    D1 size=8 addr=0000000005cb2438 tid=1
// LLMiss: why=I1_NoX size=3 addr=000000000596e8fe tid=1
// Begin DisplayList building
// End DisplayList building
// add_frame 3 000000000129fe07d (<frame_index> <frame_address>)
// add_stack 5 2 3 (<stack_index> <parent_stack> <frame_index>)

fn from_hex_str_u64(s: &str) -> result::Result<u64, ParseIntError> {
    u64::from_str_radix(s, 16)
}

fn is_not_space(chr: char) -> bool {
    chr != ' '
}
fn is_not_closing_square_bracket(chr: char) -> bool {
    chr != ']'
}

// LL cache information: <size> B, <line_size> B, <assoc>-way associative
named!(parse_llcache_info<&str, LineContent>, do_parse!(
  tag!("LL cache information: ") >>
  size: map_res!(digit, FromStr::from_str) >>
  tag!(" B, ") >>
  line_size: map_res!(digit, FromStr::from_str) >>
  tag!(" B, ") >>
  assoc: map_res!(digit, FromStr::from_str) >>
  tag!("-way associative") >>
  ( LineContent::LLCacheInfo{ size, line_size, assoc } )
));

// LLCacheSwap: new_start=<new_start> old_start=<old_start> size=<size>
named!(parse_llcache_line_swap<&str, LineContent>, do_parse!(
  tag!("LLCacheSwap: new_start=") >>
  new_start: map_res!(hex_digit, from_hex_str_u64) >>
  tag!(" old_start=") >>
  old_start: map_res!(hex_digit, from_hex_str_u64) >>
  tag!(" size=") >>
  size: map_res!(digit, FromStr::from_str) >>
  ( LineContent::LLCacheLineSwap{ new_start, old_start, size } )
));

// LLMiss: why=    D1 size=8 addr=0000000005cb2438 tid=1
// LLMiss: why=I1_NoX size=3 addr=000000000596e8fe tid=1
named!(parse_llmiss<&str, LineContent>, do_parse!(
  tag!("LLMiss: why=") >>
  why: ws!(take_while_s!(is_not_space)) >>
  tag!("size=") >>
  size: map_res!(digit, FromStr::from_str) >>
  tag!(" addr=") >>
  addr: map_res!(hex_digit, from_hex_str_u64) >>
  tag!(" tid=") >>
  tid: map_res!(digit, FromStr::from_str) >>
  ( LineContent::LLMiss{ why, size, addr, tid } )
));

// stack: 160442
named!(parse_stack_for_llmiss<&str, LineContent>, do_parse!(
  tag!("stack: ") >>
  stack_index: map_res!(digit, FromStr::from_str) >>
  ( LineContent::StackForLLMiss(stack_index) )
));

// Begin DisplayList building
named!(parse_begin_display_list<&str, LineContent>, do_parse!(
  tag!("Begin DisplayList building") >>
  ( LineContent::BeginDisplayList )
));

// End DisplayList building
named!(parse_end_display_list<&str, LineContent>, do_parse!(
  tag!("End DisplayList building") >>
  ( LineContent::EndDisplayList )
));

// [ArenaAllocator:0x976d1300] Allocating arena chunk at 0x976d7b70 with size 2048 bytes
named!(parse_allocate_arena_chunk<&str, LineContent>, do_parse!(
    char!('[') >>
    ident: take_while_s!(is_not_closing_square_bracket) >>
    char!(']') >>
    tag!(" Allocating arena chunk at 0x") >>
    chunk_start: map_res!(hex_digit, from_hex_str_u64) >>
    tag!(" with size ") >>
    chunk_size: map_res!(digit, FromStr::from_str) >>
    tag!(" bytes") >>
    ( LineContent::AllocatingArenaChunk{ ident, chunk_start, chunk_size })
));

// [ArenaAllocator:0x1ffeffdf18] Deallocating arena chunk at 0x976e4b90 with size 4096 bytes
named!(parse_deallocate_arena_chunk<&str, LineContent>, do_parse!(
    char!('[') >>
    ident: take_while_s!(is_not_closing_square_bracket) >>
    char!(']') >>
    tag!(" Deallocating arena chunk at 0x") >>
    chunk_start: map_res!(hex_digit, from_hex_str_u64) >>
    tag!(" with size ") >>
    chunk_size: map_res!(digit, FromStr::from_str) >>
    tag!(" bytes") >>
    ( LineContent::DeallocatingArenaChunk{ ident, chunk_start, chunk_size })
));

// [nsPresArena:0x97727230] has [ArenaAllocator:0x97728628]
// [PresShell:0x97727200] has [nsPresArena:0x97727230]
named!(parse_association<&str, LineContent>, do_parse!(
    char!('[') >>
    ident1: take_while_s!(is_not_closing_square_bracket) >>
    tag!("] has [") >>
    ident2: take_while_s!(is_not_closing_square_bracket) >>
    char!(']') >>
    ( LineContent::Association{ ident1, ident2 })
));

// [PresShell:0xb935a470] has URL https://people-mozilla.org/%7Ejmuizelaar/implementation-tests/dl-test.html
// [nsDisplayListBuilder:0x1ffeffd6a0] has url chrome://browser/content/browser.xul
named!(parse_extra_field<&str, LineContent>, do_parse!(
    char!('[') >>
    ident: take_while_s!(is_not_closing_square_bracket) >>
    tag!("] has ") >>
    field_name: take_while_s!(is_not_space) >>
    char!(' ') >>
    field_content: rest_s >>
    ( LineContent::ExtraField{ ident, field_name, field_content })
));

// add_frame: 3 000000000129fe07d (<frame_index> <frame_address>)
named!(parse_add_frame<&str, LineContent>, do_parse!(
  tag!("add_frame: ") >>
  index: map_res!(digit, FromStr::from_str) >>
  char!(' ') >>
  address: map_res!(hex_digit, from_hex_str_u64) >>
  ( LineContent::AddFrame{ index, address } )
));

// add_stack: 5 2 3 (<stack_index> <parent_stack> <frame_index>)
named!(parse_add_stack<&str, LineContent>, do_parse!(
  tag!("add_stack: ") >>
  index: map_res!(digit, FromStr::from_str) >>
  char!(' ') >>
  parent_stack: map_res!(digit, FromStr::from_str) >>
  char!(' ') >>
  frame: map_res!(digit, FromStr::from_str) >>
  ( LineContent::AddStack{ index, parent_stack, frame } )
));

// SharedLibsChunk: randomstuff
named!(parse_shared_libs_chunk<&str, LineContent>, do_parse!(
  tag!("SharedLibsChunk: ") >>
  chunk: rest_s >>
  ( LineContent::SharedLibsChunk(chunk) )
));

named!(parse_other<&str, LineContent>, do_parse!(
  s: rest_s >>
  (LineContent::Other(s))
));

named!(parse_line<&str, LineContent>, alt!(
    parse_llcache_info |
    parse_llcache_line_swap | parse_llmiss | parse_stack_for_llmiss |
    parse_begin_display_list |  parse_end_display_list |
    parse_allocate_arena_chunk | parse_deallocate_arena_chunk |
    parse_association | parse_extra_field |
    parse_add_frame | parse_add_stack | parse_shared_libs_chunk |
    parse_other
));

named!(parse_line_of_pid<&str, (i32, LineContent)>, do_parse!(
    tag!("==") >>
    pid: map_res!(digit, FromStr::from_str) >>
    tag!("== ") >>
    line_content: parse_line >>
    ((pid, line_content))
));

#[test]
fn test_parse_line() {
    assert_eq!(parse_line("LLCacheSwap: new_start=1ffefffe00 old_start=0 size=64"),
               IResult::Done("",
                             LineContent::LLCacheLineSwap {
                                 new_start: 0x1ffefffe00,
                                 old_start: 0x0,
                                 size: 64,
                             }));
    assert_eq!(parse_line("LLMiss: why=I1_NoX size=3 addr=000000000596e8fe tid=1"),
               IResult::Done("",
                             LineContent::LLMiss {
                                 why: "I1_NoX",
                                 size: 3,
                                 addr: 0x596e8fe,
                                 tid: 1,
                             }));

    assert_eq!(parse_line_of_pid("==16935== LLCacheSwap: new_start=1ffeffe400 old_start=0 size=64"),
               IResult::Done("",
                             (16935,
                              LineContent::LLCacheLineSwap {
                                  new_start: 0x1ffeffe400,
                                  old_start: 0x0,
                                  size: 64,
                              })));
}

fn bisection<S, T, F>(v: &Vec<S>, f: F, x: T) -> usize
    where F: Fn(&S) -> T,
          T: PartialOrd
{
    let mut low = 0;
    let mut high = v.len();

    while low < high {
        let mid = (low + high) >> 1;

        if x < f(&v[mid]) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }

    low
}

pub struct Ranges {
    r: Vec<(u64, u64)>,
}

impl Ranges {
    pub fn new() -> Ranges {
        Ranges { r: vec![] }
    }

    pub fn get(&self) -> Vec<(u64, u64)> {
        self.r.clone()
    }

    pub fn add(&mut self, mut start: u64, size: u64) {
        // return;
        let mut end = start + size;
        let insertion_index_start_start = bisection(&self.r, |&(s, _)| s, start);
        let insertion_index_start_end = bisection(&self.r, |&(_, e)| e, start);
        let insertion_index_end_start = bisection(&self.r, |&(s, _)| s, end);
        let insertion_index_end_end = bisection(&self.r, |&(_, e)| e, end);
        let mut first_removal_index = insertion_index_start_end;
        let mut after_last_removal_index = insertion_index_end_end;
        if insertion_index_start_start != insertion_index_start_end {
            // assert(insertion_index_start_start > insertion_index_start_end)
            // start falls into the range at insertion_index_start_end
            start = self.r[insertion_index_start_end].0;
            first_removal_index = insertion_index_start_end;
        } else {
            // start is before the range at insertion_index_start_start
        }
        if insertion_index_end_start != insertion_index_end_end {
            // assert(insertion_index_end_start > insertion_index_end_end)
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

    //   remove(start, size) {
    //     // console.log(this._startAddresses.slice(), this._endAddresses.slice());
    //     let end = start + size;
    //     // console.log('removing', start, end);
    //     const insertion_index_start_start = bisection(this._startAddresses, start);
    //     const insertion_index_start_end = bisection(this._endAddresses, start);
    //     const insertion_index_end_start = bisection(this._startAddresses, end);
    //     const insertion_index_end_end = bisection(this._endAddresses, end);
    //     let first_removal_index = insertion_index_start_end;
    //     let after_last_removal_index = insertion_index_end_start;
    //     let newFirstRangeStart = null;
    //     let newSecondRangeEnd = null;
    //     if (insertion_index_start_start !== insertion_index_start_end) {
    //       // assert(insertion_index_start_start > insertion_index_start_end)
    //       // start falls into the range at insertion_index_start_end
    //       newFirstRangeStart = this._startAddresses[insertion_index_start_end];
    //       if (newFirstRangeStart === start) {
    //         newFirstRangeStart = null;
    //       }
    //     } else {
    //       // start is before the range at insertion_index_start_start
    //     }
    //     if (insertion_index_end_start !== insertion_index_end_end) {
    //       // assert(insertion_index_end_start > insertion_index_end_end)
    //       // end falls into the range at insertion_index_end_end
    //       newSecondRangeEnd = this._endAddresses[insertion_index_end_end];
    //       if (newSecondRangeEnd === end) {
    //         newSecondRangeEnd = null;
    //       }
    //     } else {
    //       // end is before the range at insertion_index_end_start
    //     }
    //     if (newFirstRangeStart !== null) {
    //       if (newSecondRangeEnd !== null) {
    //         this._startAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index, newFirstRangeStart, end);
    //         this._endAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index, start, newSecondRangeEnd);
    //       } else {
    //         this._startAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index, newFirstRangeStart);
    //         this._endAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index, start);
    //       }
    //     } else {
    //        if (newSecondRangeEnd !== null) {
    //         this._startAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index, end);
    //         this._endAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index, newSecondRangeEnd);
    //        } else {
    //         this._startAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index);
    //         this._endAddresses.splice(first_removal_index, after_last_removal_index - first_removal_index);
    //        }
    //     }
    //     assertIsSorted(this._startAddresses);
    //     assertIsSorted(this._endAddresses);
    //     this._length = this._startAddresses.length;
    //     this.assert_consistency();
    //   }
}

pub struct CPUCache {
    line_size: u64,
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
    pub fn new(size: u32, line_size: u32, assoc: u32) -> CPUCache {
        let sets = ((size / line_size) / assoc) as u64;
        println!("sets: {}, sets-1: 0b{:b}", sets, sets - 1);
        CPUCache {
            // size: size as u64,
            line_size: line_size as u64,
            line_size_bits: log2(line_size).unwrap(),
            // sets,
            sets_min_1: sets - 1,
            assoc: assoc as u64,
            tags: vec![0; (size / line_size) as usize],
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

        // if evicting != 0 || set_no == 5462 {
        //     println!("evicting 0x{:x} from set number {}, set contents are now:", evicting, set_no);
        //     for i in 0..(self.assoc as usize) {
        //         println!(" - 0x{:x}", set[i] << self.line_size_bits);
        //     }
        // }

        Some(evicting)
    }

    pub fn exchange(&mut self, new_addr: u64, old_addr: u64) {
        let old_tag = old_addr >> self.line_size_bits;
        let new_tag = new_addr >> self.line_size_bits;
        let old_set_no = old_tag & self.sets_min_1;
        let new_set_no = new_tag & self.sets_min_1;
        if old_tag != 0 && old_set_no != new_set_no {
            panic!("Expected to only exchange cache lines inside the same set! old_addr={:x} new_addr={:x} old_set_no={} new_set_no={}",
                   old_addr,
                   new_addr,
                   old_set_no,
                   new_set_no);
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

    fn get_cached_ranges(&self) -> Vec<(u64, u64)> {
        let mut ranges = Ranges::new();
        for tag in &self.tags {
            if *tag != 0 {
                let start = tag << self.line_size_bits;
                ranges.add(start, self.line_size);
            }
        }
        ranges.get()
    }
}

#[allow(dead_code)]
fn print_display_list_info<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut bytes_read = 0;
    let mut ranges_read = Ranges::new();
    let mut dl_begin = 0;
    let mut in_display_list = false;
    for (line_index, line) in iter {
        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            match line_contents {
                LineContent::BeginDisplayList => {
                    dl_begin = line_index;
                    in_display_list = true;
                }
                LineContent::EndDisplayList => {
                    println!("DisplayList for {} lines from {} to {}",
                             (line_index - dl_begin),
                             dl_begin,
                             line_index);
                    let ranges_read_size = ranges_read.cumulative_size();
                    let overhead = bytes_read as f64 / ranges_read_size as f64;
                    println!("bytes_read: {}, cumulative size of ranges_read: {}, overhead: {}",
                             bytes_read,
                             ranges_read_size,
                             overhead);
                    in_display_list = false;
                    bytes_read = 0;
                    ranges_read = Ranges::new();
                }
                line_contents => {
                    if in_display_list {
                        if let LineContent::LLCacheLineSwap {
                                   new_start,
                                   old_start: _,
                                   size,
                               } = line_contents {
                            bytes_read = bytes_read + size;
                            ranges_read.add(new_start, size);
                        }
                    }
                }
            }
        };
    }
    Ok(())
}

#[allow(dead_code)]
fn print_extra_field_info<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    for (_, line) in iter {
        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            if let LineContent::ExtraField {
                       ident,
                       field_name,
                       field_content,
                   } = line_contents {
                println!("{} {} {}", ident, field_name, field_content);
            }
        };
    }
    Ok(())
}

#[allow(dead_code)]
fn print_other_lines<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    for (line_index, line) in iter {
        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            if let LineContent::Other(s) = line_contents {
                println!("{}: {}", line_index, s);
            }
        };
    }
    Ok(())
}

#[allow(dead_code)]
fn print_fork_lines<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut last_frame_index = -1;
    for (line_index, line) in iter {
        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            if let LineContent::AddFrame {
                       index: frame_index,
                       address: _,
                   } = line_contents {
                if frame_index as i32 <= last_frame_index {
                    println!("something forked at or before line {}", line_index)
                }
                last_frame_index = frame_index as i32;
            }
        };
    }
    Ok(())
}

#[allow(dead_code)]
fn print_cache_contents_at<T>(mut iter: iter::Enumerate<io::Lines<T>>,
                              at_line_index: usize)
                              -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut cache = loop {
        if let Some((_, line)) = iter.next() {
            if let IResult::Done(_,
                                 (_,
                                  LineContent::LLCacheInfo {
                                      size,
                                      line_size,
                                      assoc,
                                  })) = parse_line_of_pid(&line?) {
                break CPUCache::new(size, line_size, assoc);
            }
        } else {
            println!("Couldn't find CPU cache info, not simulating cache.");
            return Ok(());
        }
    };
    for (line_index, line) in iter {
        if line_index >= at_line_index {
            break;
        }
        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            if let LineContent::LLCacheLineSwap {
                       new_start,
                       old_start,
                       size: _,
                   } = line_contents {
                cache.exchange(new_start, old_start);
            }
        };
    }
    println!("cache ranges: {:?}", cache.get_cached_ranges());
    Ok(())
}

#[allow(dead_code)]
fn print_cache_read_overhead<T>(iter: iter::Enumerate<io::Lines<T>>,
                                from_line: usize,
                                to_line: usize)
                                -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut bytes_read = 0;
    let mut ranges_read = Ranges::new();
    for (_, line) in iter.take(to_line).skip(from_line) {
        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            if let LineContent::LLCacheLineSwap {
                       new_start,
                       old_start: _,
                       size,
                   } = line_contents {
                bytes_read = bytes_read + size;
                ranges_read.add(new_start, size);
            }
        };
    }
    let ranges_read_size = ranges_read.cumulative_size();
    let overhead = bytes_read as f64 / ranges_read_size as f64;
    println!("bytes_read: {}, cumulative size of ranges_read: {}, overhead: {}",
             bytes_read,
             ranges_read_size,
             overhead);
    Ok(())
}

fn n_times(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("{} {}", n, singular)
    } else {
        format!("{} {}", n, plural)
    }
}

#[allow(dead_code)]
fn english_concat<T>(singular: &str, plural: &str, v: &Vec<T>) -> String
    where T: Display
{
    let n = v.len();
    if n == 0 {
        format!("no {}", singular)
    } else if n == 1 {
        format!("{} {}", singular, v[0])
    } else {
        let all_but_last: Vec<_> = v.iter().take(n - 1).map(|x| x.to_string()).collect();
        format!("{} {} and {}", plural, all_but_last.join(", "), v[n - 1])
    }
}

#[allow(dead_code)]
fn print_multiple_read_ranges<T>(iter: iter::Enumerate<io::Lines<T>>,
                                 from_line: usize,
                                 to_line: usize)
                                 -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut stack_table = StackTable::new();
    let mut read_addresses = HashMap::new();
    let mut pending_cache_line_swap: Option<(u64, usize)> = None;
    let mut shared_libs_json_string = String::new();
    for (line_index, line) in iter.take(to_line) {

        if let IResult::Done(_, (_, line_contents)) = parse_line_of_pid(&line?) {
            match line_contents {
                LineContent::AddFrame { index, address } => {
                    stack_table.add_frame(index, address);
                }
                LineContent::AddStack {
                    index,
                    parent_stack,
                    frame,
                } => {
                    stack_table.add_stack(index, parent_stack, frame);
                }
                LineContent::SharedLibsChunk(ref json_string_chunk) => {
                    shared_libs_json_string.push_str(json_string_chunk);
                }
                _ => {}
            }
            if line_index >= from_line {
                match line_contents {
                    LineContent::LLCacheLineSwap {
                        new_start,
                        old_start: _,
                        size: _,
                    } => {
                        pending_cache_line_swap = Some((new_start, line_index));
                    }
                    LineContent::StackForLLMiss(stack_index) => {
                        if let Some((cache_miss_addr, cache_miss_line_index)) =
                            pending_cache_line_swap {
                            read_addresses
                                .entry(cache_miss_addr)
                                .or_insert(Vec::new())
                                .push((cache_miss_line_index, stack_index));
                            pending_cache_line_swap = None;
                        }
                    }
                    _ => {}
                }
            }
        };
    }

    let mut multiple_reads: Vec<(&u64, &Vec<(usize, usize)>)> = read_addresses
        .iter()
        .filter(|&(_, v)| v.len() > 1)
        .collect();
    multiple_reads.sort_by_key(|&(_, v)| -(v.len() as isize));

    println!("Shared libraries: {}", shared_libs_json_string);
    match SharedLibraries::from_json_string(shared_libs_json_string) {
        Ok(shared_libraries) => {
            stack_table.set_libs(shared_libraries);
        }
        Err(e) => {
            println!("error during json parsing: {:?}", e);
        }
    }

    println!("Read {} cache-line sized memory ranges at least twice.",
             multiple_reads.len());
    for &(addr, reads) in multiple_reads.iter().take(25) {
        println!("Read cache line at address 0x{:x} {}:",
                 addr,
                 n_times(reads.len(), "time", "times"));
        for (i, &(line_index, stack_index)) in reads.iter().enumerate() {
            println!(" ({}) At line {}:", i + 1, line_index);
            stack_table.print_stack(stack_index);
        }
    }

    Ok(())
}

struct StackEntry {
    parent_stack: usize,
    frame: usize,
}

pub struct StackTable {
    frames: Vec<u64>,
    stacks: Vec<StackEntry>,
    libs: Option<SharedLibraries>
}

impl StackTable {
    pub fn new() -> StackTable {
        StackTable {
            frames: Vec::new(),
            stacks: Vec::new(),
            libs: None,
        }
    }

    pub fn add_frame(&mut self, index: usize, address: u64) {
        assert!(index == self.frames.len(), "unexpected frame index");
        self.frames.push(address);
    }

    pub fn add_stack(&mut self, index: usize, parent_stack: usize, frame: usize) {
        assert!(index == self.stacks.len(), "unexpected stack index");
        assert!(parent_stack < index || parent_stack == 0,
                "can't refer to parent stacks that I haven't seen yet");
        assert!(frame < self.frames.len(),
                "can't refer to frames that I haven't seen yet");
        self.stacks
            .push(StackEntry {
                      parent_stack,
                      frame,
                  });
    }

    fn frame_index_list_for_stack(&self, stack: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut stack_index = stack;
        while stack_index != 0 {
            let StackEntry {
                frame,
                parent_stack,
            } = self.stacks[stack_index];
            result.push(frame);
            stack_index = parent_stack;
        }
        result.reverse();
        result
    }

    pub fn print_stack(&self, stack: usize) {
        for frame in self.frame_index_list_for_stack(stack) {
            let address = self.frames[frame];
            if let &Some(ref libs) = &self.libs {
                if let Some(lib) = libs.lib_for_address(address) {
                    let relative_address = address - lib.start;
                    println!("  0x{:016x} [{} + 0x{:x}]", address, lib.name, relative_address);
                } else {
                    println!("  0x{:016x} [unknown binary]", address);
                }
            } else {
                println!("  0x{:016x}", address);
            }
        }
    }

    pub fn set_libs(&mut self, libs: SharedLibraries) {
        self.libs = Some(libs);
    }
}

pub struct SymbolTable {}

impl SymbolTable {
    pub fn from_breakpad_symbol_dump<T: BufRead>(buffer: T) -> Result<u64, io::Error> {
        let iter = buffer.lines().enumerate();
        // print_display_list_info(iter)?;
        // print_cache_contents_at(iter, 67317106)?;
        // print_cache_contents_at(iter, cache, 58256278)?;
        // print_extra_field_info(iter)?;
        // print_other_lines(iter)?;
        // print_fork_lines(iter)?;
        // print_cache_read_overhead(iter, 67317106, 67500260)?;
        print_multiple_read_ranges(iter, 100842605, 101204593)?;
        Ok(6)
    }
}
