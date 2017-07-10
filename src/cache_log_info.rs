use std::io;
use std::iter;
use std::collections::HashMap;
use std::fmt::Display;
use cache_log_parsing::{parse_line_of_pid, LineContent};
use ranges::Ranges;
use cpucache::CPUCache;
use stack_table::StackTable;
use shared_libraries::SharedLibraries;
use arenas::Arenas;

#[allow(dead_code)]
pub fn print_display_list_info<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut bytes_read = 0;
    let mut ranges_read = Ranges::new();
    let mut dl_begin = 0;
    let mut in_display_list = false;
    for (line_index, line) in iter {
        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
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
pub fn print_extra_field_info<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    for (_, line) in iter {
        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
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
pub fn print_other_lines<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    for (line_index, line) in iter {
        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
            if let LineContent::Other(s) = line_contents {
                println!("{}: {}", line_index, s);
            }
        };
    }
    Ok(())
}

#[allow(dead_code)]
pub fn print_fork_lines<T>(iter: iter::Enumerate<io::Lines<T>>) -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut last_frame_index = -1;
    for (line_index, line) in iter {
        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
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
pub fn print_cache_contents_at<T>(mut iter: iter::Enumerate<io::Lines<T>>,
                                  at_line_index: usize)
                                  -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut cache = loop {
        if let Some((_, line)) = iter.next() {
            if let Some((_,
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
        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
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
pub fn print_cache_read_overhead<T>(iter: iter::Enumerate<io::Lines<T>>,
                                    from_line: usize,
                                    to_line: usize)
                                    -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut bytes_read = 0;
    let mut ranges_read = Ranges::new();
    for (_, line) in iter.take(to_line).skip(from_line) {
        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
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

fn type_from_ident(ident: &str) -> &str {
    if let Some(colon_index) = ident.find(':') {
        let (before_colon, _) = ident.split_at(colon_index);
        before_colon
    } else {
        "Unknown"
    }
}

struct ArenaInfoCollector {
    arenas: Arenas,
    bytes_read: HashMap<String, u64>,
}

impl ArenaInfoCollector {
    pub fn new() -> ArenaInfoCollector {
        ArenaInfoCollector {
            arenas: Arenas::new(),
            bytes_read: HashMap::new(),
        }
    }

    pub fn process_line(&mut self, line_content: &LineContent) {
        match line_content {
            &LineContent::AllocatingArenaChunk {
                 ident,
                 chunk_start,
                 chunk_size,
             } => {
                self.arenas
                    .allocate_chunk(ident, chunk_start, chunk_size);
            }
            &LineContent::DeallocatingArenaChunk {
                 ident,
                 chunk_start,
                 chunk_size,
             } => {
                self.arenas
                    .deallocate_chunk(ident, chunk_start, chunk_size);
            }
            &LineContent::Association { ident1, ident2 } => {
                let type1 = type_from_ident(ident1);
                let type2 = type_from_ident(ident2);
                if type1 == "ArenaAllocator" {
                    self.arenas
                        .associate_arena_with_thing(ident1, type2, ident2);
                } else if type2 == "ArenaAllocator" {
                    self.arenas
                        .associate_arena_with_thing(ident2, type1, ident1);
                } else {
                    self.arenas
                        .associate_thing_with_thing(type1, ident1, type2, ident2);
                }
            }
            &LineContent::ExtraField {
                 ident,
                 field_name,
                 field_content,
             } => {
                self.arenas
                    .set_thing_property(ident, field_name, field_content);
            }
            _ => {}
        }
    }

    pub fn arenas(&self) -> &Arenas {
        &self.arenas
    }

    pub fn into_arenas(self) -> Arenas {
        self.arenas
    }
}

struct StackInfoCollector {
    stack_table: StackTable,
    shared_libs_json_string: String,
}

impl StackInfoCollector {
    pub fn new() -> StackInfoCollector {
        StackInfoCollector {
            stack_table: StackTable::new(),
            shared_libs_json_string: String::new(),
        }
    }

    pub fn process_line(&mut self, line_content: &LineContent) {
        match line_content {
            &LineContent::AddFrame { index, address } => {
                self.stack_table.add_frame(index, address);
            }
            &LineContent::AddStack {
                 index,
                 parent_stack,
                 frame,
             } => {
                self.stack_table.add_stack(index, parent_stack, frame);
            }
            &LineContent::SharedLibsChunk(ref json_string_chunk) => {
                self.shared_libs_json_string.push_str(json_string_chunk);
            }
            _ => {}
        }
    }

    pub fn get_stack_table(self) -> StackTable {
        let StackInfoCollector {
            mut stack_table,
            shared_libs_json_string,
        } = self;
        match SharedLibraries::from_json_string(shared_libs_json_string) {
            Ok(shared_libraries) => {
                stack_table.set_libs(shared_libraries);
            }
            Err(e) => {
                println!("error during json parsing: {:?}", e);
            }
        }
        stack_table
    }
}

struct AddressReadEvent {
    line_index: usize,
    stack: usize,
}

struct AddressReads {
    reads_per_address: HashMap<u64, Vec<AddressReadEvent>>,
}

fn into_histogram<T>(mut v: Vec<T>) -> Vec<(T, usize)>
    where T: Ord + Copy
{
    if v.is_empty() {
        return Vec::new();
    }

    v.sort_by(|a, b| b.cmp(a));
    let mut result = Vec::new();
    let mut cur_val = v[0];
    let mut cur_val_count = 1;
    for val in v.into_iter().skip(1) {
        if cur_val == val {
            cur_val_count += 1;
        } else {
            result.push((cur_val, cur_val_count));
            cur_val = val;
            cur_val_count = 1;
        }
    }
    result.push((cur_val, cur_val_count));
    result
}

impl AddressReads {
    pub fn new() -> AddressReads {
        AddressReads { reads_per_address: HashMap::new() }
    }

    pub fn add_read(&mut self, address: u64, line_index: usize, stack: usize) {
        self.reads_per_address
            .entry(address)
            .or_insert(Vec::new())
            .push(AddressReadEvent { line_index, stack });
    }

    pub fn multiple_reads_count(&self) -> usize {
        self.reads_per_address
            .iter()
            .filter(|&(_, v)| v.len() > 1)
            .count()
    }

    pub fn histogram(&self) -> (Vec<(usize, usize)>, usize) {
        let read_counts: Vec<usize> = self.reads_per_address
            .values()
            .map(|v| v.len())
            .collect();
        (into_histogram(read_counts), self.reads_per_address.len())
    }

    pub fn print_histogram(&self) {
        let (histogram, total_address_count) = self.histogram();
        for (read_count, read_count_count) in histogram {
            println!("        {} cache-line sized memory ranges were read {} ({:.0}%)",
                     read_count_count,
                     n_times(read_count, "time", "times"),
                     100f32 * read_count_count as f32 / total_address_count as f32);
        }
    }
}

struct ArenaAddressReads {
    address_reads_per_arena_ident: HashMap<String, (AddressReads, u64)>,
}

impl ArenaAddressReads {
    pub fn new() -> ArenaAddressReads {
        ArenaAddressReads { address_reads_per_arena_ident: HashMap::new() }
    }

    pub fn add_read(&mut self,
                    arena_ident: &str,
                    address: u64,
                    size: u64,
                    line_index: usize,
                    stack: usize) {
        let arena = self.address_reads_per_arena_ident
            .entry(arena_ident.to_owned())
            .or_insert((AddressReads::new(), 0u64));
        arena.0.add_read(address, line_index, stack);
        arena.1 += size;
    }

    pub fn into_arenas_sorted_by_most_bytes_read(self) -> Vec<(String, (AddressReads, u64))> {
        let mut arenas_read: Vec<(String, (AddressReads, u64))> =
            self.address_reads_per_arena_ident.into_iter().collect();
        arenas_read.sort_by_key(|&(_, (_, s))| -(s as isize));
        arenas_read
    }
}

#[allow(dead_code)]
pub fn print_multiple_read_ranges<T>(iter: iter::Enumerate<io::Lines<T>>,
                                     from_line: usize,
                                     to_line: usize)
                                     -> Result<(), io::Error>
    where T: io::BufRead
{
    let mut stack_info = StackInfoCollector::new();
    let mut arena_info = ArenaInfoCollector::new();
    let mut address_reads = AddressReads::new();
    let mut pending_cache_line_swaps: Vec<(u64, u64, usize)> = Vec::new();

    let mut outside_arena_reads = AddressReads::new();
    let mut arena_reads = ArenaAddressReads::new();
    let mut bytes_read_outside_arena = 0u64;
    let mut total_bytes_read = 0u64;

    for (line_index, line) in iter.take(to_line) {

        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
            stack_info.process_line(&line_contents);
            arena_info.process_line(&line_contents);
            if line_index >= from_line {
                match line_contents {
                    LineContent::LLCacheLineSwap {
                        new_start,
                        old_start: _,
                        size,
                    } => {
                        pending_cache_line_swaps.push((new_start, size, line_index));
                    }
                    LineContent::StackForLLMiss(stack_index) => {
                        for (cache_miss_addr, size, cache_miss_line_index) in
                            pending_cache_line_swaps.drain(..) {
                            address_reads.add_read(cache_miss_addr,
                                                   cache_miss_line_index,
                                                   stack_index);

                            if let Some(arena_ident) =
                                arena_info
                                    .arenas()
                                    .arena_covering_address(cache_miss_addr) {
                                arena_reads.add_read(&arena_ident,
                                                     cache_miss_addr,
                                                     size,
                                                     cache_miss_line_index,
                                                     stack_index);
                            } else {
                                outside_arena_reads.add_read(cache_miss_addr,
                                                             cache_miss_line_index,
                                                             stack_index);
                                bytes_read_outside_arena += size;
                            }
                            total_bytes_read += size;
                        }
                    }
                    _ => {}
                }
            }
        };
    }

    let mut stack_table = stack_info.get_stack_table();

    println!("Read {} cache-line sized memory ranges at least twice.",
             address_reads.multiple_reads_count());
    address_reads.print_histogram();
    println!("");

    // let mut multiple_reads: Vec<(&u64, &Vec<(usize, usize)>)> = read_addresses
    //     .iter()
    //     .filter(|&(_, v)| v.len() > 1)
    //     .collect();
    // multiple_reads.sort_by_key(|&(_, v)| -(v.len() as isize));

    // for &(addr, reads) in multiple_reads.iter().take(25) {
    //     println!("Read cache line at address 0x{:x} {}:",
    //              addr,
    //              n_times(reads.len(), "time", "times"));
    //     for (i, &(line_index, stack_index)) in reads.iter().enumerate() {
    //         println!(" ({}) At line {}:", i + 1, line_index);
    //         stack_table.print_stack(stack_index);
    //     }
    // }

    println!("Read {} ({:.0}%) bytes outside any arena.",
             bytes_read_outside_arena,
             100f64 * bytes_read_outside_arena as f64 / total_bytes_read as f64);
    outside_arena_reads.print_histogram();

    let arenas_read = arena_reads.into_arenas_sorted_by_most_bytes_read();
    let mut arenas = arena_info.into_arenas();
    for (arena, (arena_address_reads, bytes)) in arenas_read.into_iter() {
        println!("Read {} bytes ({:.0}%) from arena {}:",
                 bytes,
                 100f64 * bytes as f64 / total_bytes_read as f64,
                 &arena);
        println!("    {}", arenas.arena_description(&arena));
        arena_address_reads.print_histogram();
    }

    Ok(())
}
