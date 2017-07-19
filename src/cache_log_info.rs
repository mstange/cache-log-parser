use std::iter;
use std::collections::HashMap;
use std::fmt::Display;
use cache_log_parsing::{parse_line_of_pid, LineContent};
use ranges::Ranges;
use cpucache::CPUCache;
use stack_table::StackTable;
use shared_libraries::SharedLibraries;
use arenas::Arenas;
use profile::ProfileBuilder;
use rand::{self, Rng};
use pretty_bytes::converter::convert;
use fixed_circular_buffer::CircularBuffer;

#[derive(Debug)]
struct PIDs {
    pids: Vec<(i32, usize)>,
}

impl PIDs {
    pub fn increment(&mut self, pid: i32) {
        for &mut (pid_, ref mut line_count) in self.pids.iter_mut() {
            if pid_ == pid {
                *line_count += 1;
                return;
            }
        }
        self.pids.push((pid, 1));
    }
}

#[allow(dead_code)]
pub fn print_process_info<T>(iter: T)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    let mut pids = PIDs { pids: Vec::new() };
    for (_, line) in iter {
        if let Some((pid, _)) = parse_line_of_pid(&line) {
            pids.increment(pid);
        }
    }
    let mut pid_iter = pids.pids.into_iter();
    if let Some((parent_process_pid, line_count)) = pid_iter.next() {
        println!(
            "Parent process: {} ({} log lines)",
            parent_process_pid,
            line_count
        );
        let mut remaining_pids: Vec<(i32, usize)> = pid_iter.collect();
        remaining_pids.sort_by(|&(_, ref a), &(_, b)| b.cmp(a));
        let mut pid_iter = remaining_pids.into_iter().peekable();
        if let Some((primary_content_process, line_count)) = pid_iter.next() {
            println!(
                "Primary content process: {} ({} log lines)",
                primary_content_process,
                line_count
            );
            if let Some(_) = pid_iter.peek() {
                println!("Other child processes:");
                for (other_pid, line_count) in pid_iter {
                    println!(" - {} ({} log lines)", other_pid, line_count);
                }
            }
        } else {
            println!("No child process found.");
        }
    } else {
        println!("Did not find any processes in the log.");
    }
}

struct DisplayListBuildingSection {
    start_line_index: usize,
    end_line_index: Option<usize>,
    reads_info: ReadsCollector,
}

impl DisplayListBuildingSection {
    pub fn new(start_line_index: usize) -> DisplayListBuildingSection {
        DisplayListBuildingSection {
            start_line_index,
            end_line_index: None,
            reads_info: ReadsCollector::new(),
        }
    }

    pub fn needs_more_lines(&self) -> bool {
        self.reads_info.needs_more_lines()
    }

    pub fn process_line(&mut self, line_index: usize, line_contents: &LineContent) {
        assert!(line_index >= self.start_line_index);
        self.reads_info.process_line(
            line_index,
            self.end_line_index == None,
            line_contents,
        );
    }

    pub fn found_section_end(&mut self, line_index: usize) {
        self.end_line_index = Some(line_index);
    }

    pub fn print_info(self) {
        let mut bytes_read: u64 = 0;
        let mut bytes_used: u64 = 0;
        let mut ranges_read = Ranges::new();

        for CacheLineRead {
            line_index: _,
            address,
            size,
            used_bytes,
            stack: _,
        } in self.reads_info.into_reads()
        {
            bytes_read += size as u64;
            bytes_used += used_bytes.unwrap_or(size) as u64;
            ranges_read.add(address, size as u64);
        }

        println!(
            "  - DisplayList section which contains {} of memory reads",
            convert(bytes_read as f64)
        );
        if let Some(end_line_index) = self.end_line_index {
            println!(
                "      - frome line {} to line {} ({} lines total)",
                self.start_line_index,
                end_line_index,
                (end_line_index - self.start_line_index)
            );
        } else {
            println!(
                "      - starting at line {}, unfinished",
                self.start_line_index
            );
        }

        let ranges_read_size = ranges_read.cumulative_size();
        let multi_read_overhead = (bytes_read as f64 / ranges_read_size as f64 - 1.0) * 100.0;
        let cache_line_overhead = (bytes_read as f64 / bytes_used as f64 - 1.0) * 100.0;
        println!(
            "      - read {} bytes from memory into the LL cache in total",
            bytes_read,
        );
        println!(
            "      - read {} bytes of unique address ranges into the cache",
            ranges_read_size,
        );
        println!(
            "         => {:.0}% overhead due to memory ranges that were read more than once",
            multi_read_overhead
        );
        println!(
            "      - accessed {} bytes",
            bytes_used,
        );
        println!(
            "         => {:.0}% overhead from unused parts of cache lines",
            cache_line_overhead
        );
        println!("");
    }
}

#[allow(dead_code)]
pub fn print_display_list_info<T>(pid: i32, iter: T)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    let mut pending_dlb_sections: Vec<DisplayListBuildingSection> = Vec::new();
    let mut current_dlb_section: Option<DisplayListBuildingSection> = None;
    for (line_index, line) in iter {
        if let Some((p, line_contents)) = parse_line_of_pid(&line) {
            if p != pid {
                continue;
            }
            match line_contents {
                LineContent::BeginDisplayList => {
                    current_dlb_section = Some(DisplayListBuildingSection::new(line_index));
                }
                LineContent::EndDisplayList => {
                    current_dlb_section
                        .as_mut()
                        .expect("Unbalanced End DisplayList")
                        .found_section_end(line_index);
                    pending_dlb_sections.push(current_dlb_section.take().unwrap());
                }
                line_contents => {
                    if let Some(current_dlb_section) = current_dlb_section.as_mut() {
                        current_dlb_section.process_line(line_index, &line_contents);
                    }
                    pending_dlb_sections = pending_dlb_sections
                        .into_iter()
                        .flat_map(|mut section| {
                            section.process_line(line_index, &line_contents);
                            if section.needs_more_lines() {
                                return Some(section);
                            }

                            section.print_info();
                            None
                        })
                        .collect();
                }
            }
        };
    }
    if !pending_dlb_sections.is_empty() {
        println!(
            "Have sections for which I don't know all the bytes_used information. Going to assume that the full cache line was used."
        );
        for section in pending_dlb_sections.into_iter() {
            section.print_info();
        }
    }
}

#[derive(Debug)]
struct CacheLineRead {
    line_index: usize,
    address: u64,
    size: u8,
    used_bytes: Option<u8>,
    stack: Option<usize>,
}

struct ReadsCollector {
    reads: Vec<CacheLineRead>,
    reads_with_pending_used_bytes: HashMap<u64, usize>,
    reads_with_pending_stacks: Vec<usize>,
}

impl ReadsCollector {
    pub fn new() -> ReadsCollector {
        ReadsCollector {
            reads: Vec::new(),
            reads_with_pending_used_bytes: HashMap::new(),
            reads_with_pending_stacks: Vec::new(),
        }
    }

    pub fn needs_more_lines(&self) -> bool {
        !self.reads_with_pending_used_bytes.is_empty() || !self.reads_with_pending_stacks.is_empty()
    }

    pub fn into_reads(self) -> Vec<CacheLineRead> {
        self.reads
    }

    pub fn process_line(
        &mut self,
        line_index: usize,
        within_interesting_section: bool,
        line_contents: &LineContent,
    ) {
        match line_contents {
            &LineContent::LLCacheLineSwap {
                new_start,
                old_start,
                size,
                used_bytes,
            } => {
                if let Some(read_index) = self.reads_with_pending_used_bytes.remove(&old_start) {
                    self.reads[read_index].used_bytes = used_bytes;
                }
                if within_interesting_section {
                    let next_read_index = self.reads.len();
                    self.reads_with_pending_used_bytes.insert(
                        new_start,
                        next_read_index,
                    );
                    self.reads_with_pending_stacks.push(next_read_index);
                    self.reads.push(CacheLineRead {
                        line_index,
                        address: new_start,
                        size,
                        used_bytes: None,
                        stack: None,
                    });
                }
            }
            &LineContent::StackForLLMiss(stack) => {
                for read_index in self.reads_with_pending_stacks.drain(..) {
                    self.reads[read_index].stack = Some(stack);
                }
            }
            _ => {}
        }
    }
}

#[allow(dead_code)]
pub fn print_cache_line_wastage<T>(pid: i32, iter: T, from_line: usize, to_line: usize)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    let mut stack_info = StackInfoCollector::new();
    let mut reads_info = ReadsCollector::new();
    for (line_index, line) in iter {
        if let Some((p, line_contents)) = parse_line_of_pid(&line) {
            if p != pid {
                continue;
            }
            if line_index < to_line {
                stack_info.process_line(&line_contents);
            } else if !reads_info.needs_more_lines() {
                break;
            }
            if line_index >= from_line {
                reads_info.process_line(line_index, line_index < to_line, &line_contents);
            }
        }
    }

    let reads = reads_info.into_reads();

    let bytes_per_ms: u32 = 1024;
    let bytes_per_sample: u32 = 256;

    let mut stack_table = stack_info.get_stack_table();

    let mut read_bytes_profile_builder = ProfileBuilder::new(
        stack_table.clone(),
        bytes_per_sample as f64 / bytes_per_ms as f64,
    );

    let mut used_bytes_profile_builder = ProfileBuilder::new(
        stack_table.clone(),
        bytes_per_sample as f64 / bytes_per_ms as f64,
    );

    let mut wasted_bytes_profile_builder = ProfileBuilder::new(
        stack_table.clone(),
        bytes_per_sample as f64 / bytes_per_ms as f64,
    );

    let mut rng = rand::weak_rng();
    let mut read_bytes_cumulative = 0;
    let mut used_bytes_cumulative = 0;
    let mut wasted_bytes_cumulative = 0;

    let mut wasted_bytes_cumulative_per_stack = HashMap::new();

    for CacheLineRead {
        line_index: _,
        address: _,
        size: read_bytes,
        used_bytes,
        stack,
    } in reads.into_iter()
    {
        if let (Some(used_bytes), Some(stack)) = (used_bytes, stack) {
            let wasted_bytes = read_bytes - used_bytes;
            *wasted_bytes_cumulative_per_stack.entry(stack).or_insert(
                0u64,
            ) += wasted_bytes as u64;

            if rng.next_u32() % bytes_per_sample < read_bytes as u32 {
                read_bytes_profile_builder.add_sample(
                    stack,
                    read_bytes_cumulative as f64 /
                        bytes_per_ms as f64,
                );
            }
            if rng.next_u32() % bytes_per_sample < used_bytes as u32 {
                used_bytes_profile_builder.add_sample(
                    stack,
                    used_bytes_cumulative as f64 /
                        bytes_per_ms as f64,
                );
            }
            if rng.next_u32() % bytes_per_sample < wasted_bytes as u32 {
                wasted_bytes_profile_builder.add_sample(
                    stack,
                    wasted_bytes_cumulative as f64 /
                        bytes_per_ms as f64,
                );
            }
            read_bytes_cumulative += read_bytes as u64;
            used_bytes_cumulative += used_bytes as u64;
            wasted_bytes_cumulative += wasted_bytes as u64;
        }
    }
    read_bytes_profile_builder
        .save_to_file("/home/mstange/Desktop/read_bytes_profile.sps.json")
        .expect("JSON file writing went wrong");
    used_bytes_profile_builder
        .save_to_file("/home/mstange/Desktop/used_bytes_profile.sps.json")
        .expect("JSON file writing went wrong");
    wasted_bytes_profile_builder
        .save_to_file("/home/mstange/Desktop/wasted_bytes_profile.sps.json")
        .expect("JSON file writing went wrong");
    let mut wasted_bytes_cumulative_per_stack: Vec<(usize, u64)> =
        wasted_bytes_cumulative_per_stack.into_iter().collect();
    wasted_bytes_cumulative_per_stack.sort_by(|&(_, ref wb1), &(_, wb2)| wb2.cmp(wb1));
    for (stack, wasted_bytes) in wasted_bytes_cumulative_per_stack.into_iter().take(10) {
        println!(
            "Wasted {} at stack {}.",
            convert(wasted_bytes as f64),
            stack
        );
        stack_table.print_stack(stack, 4);
    }
}

#[allow(dead_code)]
pub fn print_other_lines<T>(iter: T)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    for (_, line) in iter {
        if let Some((_, line_contents)) = parse_line_of_pid(&line) {
            if let LineContent::Other(_) = line_contents {
                println!("{}", line);
            }
        }
    }
}

#[allow(dead_code)]
pub fn print_surrounding_lines<T>(pid: i32, iter: T, line_index: usize, context: usize)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    let mut surrounding_lines = CircularBuffer::from(vec!["".to_owned(); context + 1 + context]);
    let mut remaining_lines = context + 1;
    for (li, line) in iter {
        if let Some((p, _)) = parse_line_of_pid(&line) {
            if p != pid {
                continue;
            }
            surrounding_lines.queue(line.clone());
            if li >= line_index {
                remaining_lines -= 1;
                if remaining_lines == 0 {
                    break;
                }
            }
        }
    }
    println!("surrounding_lines:");
    let mut surrounding_lines: Vec<(usize, &String)> =
        surrounding_lines.iter().enumerate().collect();
    surrounding_lines.reverse();
    for (i, line) in surrounding_lines.into_iter() {
        if i == context {
            println!(" > {}", line);
        } else {
            println!("   {}", line);
        }
    }
}

fn find_cpucache_info<T>(pid: i32, iter: &mut T) -> Option<CPUCache>
where
    T: iter::Iterator<Item = (usize, String)>,
{
    loop {
        if let Some((_, line)) = iter.next() {
            if let Some((p, ref line_contents)) = parse_line_of_pid(&line) {
                if p != pid {
                    continue;
                }
                if let &LineContent::LLCacheInfo {
                    size,
                    line_size,
                    assoc,
                } = line_contents
                {
                    return Some(CPUCache::new(size, line_size, assoc));
                }
            }
        } else {
            println!("Couldn't find CPU cache info, not simulating cache.");
            return None;
        }
    }

}

#[allow(dead_code)]
pub fn print_cache_contents_at<T>(pid: i32, mut iter: T, at_line_index: usize)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    if let Some(mut cache) = find_cpucache_info(pid, &mut iter) {
        for (line_index, line) in iter {
            if line_index >= at_line_index {
                break;
            }
            if let Some((p, line_contents)) = parse_line_of_pid(&line) {
                if p != pid {
                    continue;
                }
                if let LineContent::LLCacheLineSwap {
                    new_start,
                    old_start,
                    size: _,
                    used_bytes: _,
                } = line_contents
                {
                    cache.exchange(new_start, old_start);
                }
            };
        }
        println!("cache ranges: {:?}", cache.get_cached_ranges());
    }
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
where
    T: Display,
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
}

impl ArenaInfoCollector {
    pub fn new() -> ArenaInfoCollector {
        ArenaInfoCollector { arenas: Arenas::new() }
    }

    pub fn process_line(&mut self, line_content: &LineContent) {
        match line_content {
            &LineContent::AllocatingArenaChunk {
                ident,
                chunk_start,
                chunk_size,
            } => {
                self.arenas.allocate_chunk(ident, chunk_start, chunk_size);
            }
            &LineContent::DeallocatingArenaChunk {
                ident,
                chunk_start,
                chunk_size,
            } => {
                self.arenas.deallocate_chunk(ident, chunk_start, chunk_size);
            }
            &LineContent::Association { ident1, ident2 } => {
                let type1 = type_from_ident(ident1);
                let type2 = type_from_ident(ident2);
                if type1 == "ArenaAllocator" {
                    self.arenas.associate_arena_with_thing(
                        ident1,
                        type2,
                        ident2,
                    );
                } else if type2 == "ArenaAllocator" {
                    self.arenas.associate_arena_with_thing(
                        ident2,
                        type1,
                        ident1,
                    );
                } else {
                    self.arenas.associate_thing_with_thing(
                        type1,
                        ident1,
                        type2,
                        ident2,
                    );
                }
            }
            &LineContent::ExtraField {
                ident,
                field_name,
                field_content,
            } => {
                self.arenas.set_thing_property(
                    ident,
                    field_name,
                    field_content,
                );
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
        if shared_libs_json_string.is_empty() {
            println!("Couldn't find any SharedLibrary information in the log.");
        } else {
            match SharedLibraries::from_json_string(shared_libs_json_string) {
                Ok(shared_libraries) => {
                    stack_table.set_libs(shared_libraries);
                }
                Err(e) => {
                    println!("error during json parsing: {:?}", e);
                }
            }
        }
        stack_table
    }
}

#[derive(Clone)]
struct AddressReadOrEvictEvent {
    line_index: usize,
    stack: usize,
}

#[derive(Clone)]
struct AddressReadWithPotentialEviction {
    read: AddressReadOrEvictEvent,
    eviction: Option<AddressReadOrEvictEvent>,
}

impl AddressReadWithPotentialEviction {
    pub fn new(line_index: usize, stack: usize) -> AddressReadWithPotentialEviction {
        AddressReadWithPotentialEviction {
            read: AddressReadOrEvictEvent { line_index, stack },
            eviction: None,
        }
    }
}

struct AddressReads {
    reads_per_address: HashMap<u64, Vec<AddressReadWithPotentialEviction>>,
}

fn into_histogram<T>(mut v: Vec<T>) -> Vec<(T, usize)>
where
    T: Ord + Copy,
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
            .push(AddressReadWithPotentialEviction::new(line_index, stack));
    }

    pub fn add_eviction(&mut self, address: u64, line_index: usize, stack: usize) {
        if let Some(events) = self.reads_per_address.get_mut(&address) {
            if let Some(last_read) = events.last_mut() {
                last_read.eviction = Some(AddressReadOrEvictEvent { line_index, stack });
            }
        }
    }

    pub fn multiple_reads_count(&self) -> usize {
        self.reads_per_address
            .iter()
            .filter(|&(_, v)| v.len() > 1)
            .count()
    }

    pub fn histogram(&self) -> (Vec<(usize, usize)>, usize) {
        let read_counts: Vec<usize> = self.reads_per_address.values().map(|v| v.len()).collect();
        (into_histogram(read_counts), self.reads_per_address.len())
    }

    pub fn print_histogram(&self) {
        let (histogram, total_address_count) = self.histogram();
        for (read_count, read_count_count) in histogram {
            println!(
                "    {} cache-line sized memory ranges were read {} ({:.0}%)",
                read_count_count,
                n_times(read_count, "time", "times"),
                100f32 * read_count_count as f32 / total_address_count as f32
            );
        }
    }

    pub fn print_top_n_stacks(&self, n: usize, stack_table: &mut StackTable) {
        let mut reads: Vec<(u64, Vec<AddressReadWithPotentialEviction>)> = self.reads_per_address
            .iter()
            .map(|(address, address_read_events)| {
                (*address, (*address_read_events).clone())
            })
            .collect();
        reads.sort_by(|&(_, ref v1), &(_, ref v2)| v2.len().cmp(&v1.len()));
        for (addr, reads) in reads.into_iter().take(n) {
            println!(
                "      * Read cache line at address 0x{:x} {}:",
                addr,
                n_times(reads.len(), "time", "times")
            );
            for (i,
                 AddressReadWithPotentialEviction {
                     read: AddressReadOrEvictEvent { line_index, stack },
                     eviction,
                 }) in reads.into_iter().enumerate()
            {
                println!("          {} At line {}:", i + 1, line_index);
                println!("");
                stack_table.print_stack(stack, 12);
                println!("");
                if let Some(AddressReadOrEvictEvent { line_index, stack }) = eviction {
                    println!(
                        "            This cache line was subsequently evicted at line {}:",
                        line_index
                    );
                    println!("");
                    stack_table.print_stack(stack, 12);
                    println!("");
                } else {
                    println!("            (No eviction)");
                }
            }
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

    pub fn add_read(
        &mut self,
        arena_ident: &str,
        address: u64,
        size: u8,
        line_index: usize,
        stack: usize,
    ) {
        let arena = self.address_reads_per_arena_ident
            .entry(arena_ident.to_owned())
            .or_insert((AddressReads::new(), 0u64));
        arena.0.add_read(address, line_index, stack);
        arena.1 += size as u64;
    }

    pub fn add_eviction(
        &mut self,
        arena_ident: &str,
        address: u64,
        line_index: usize,
        stack: usize,
    ) {
        let arena = self.address_reads_per_arena_ident
            .entry(arena_ident.to_owned())
            .or_insert((AddressReads::new(), 0u64));
        arena.0.add_eviction(address, line_index, stack);
    }

    pub fn into_arenas_sorted_by_most_bytes_read(self) -> Vec<(String, (AddressReads, u64))> {
        let mut arenas_read: Vec<(String, (AddressReads, u64))> =
            self.address_reads_per_arena_ident.into_iter().collect();
        arenas_read.sort_by_key(|&(_, (_, s))| -(s as isize));
        arenas_read
    }
}

#[allow(dead_code)]
pub fn print_multiple_read_ranges<T>(pid: i32, iter: T, from_line: usize, to_line: usize)
where
    T: iter::Iterator<Item = (usize, String)>,
{
    let mut stack_info = StackInfoCollector::new();
    let mut arena_info = ArenaInfoCollector::new();
    let mut address_reads = AddressReads::new();
    let mut pending_cache_line_swaps: Vec<(u64, u64, u8, usize)> = Vec::new();

    let mut outside_arena_reads = AddressReads::new();
    let mut arena_reads = ArenaAddressReads::new();
    let mut bytes_read_outside_arena = 0u64;
    let mut total_bytes_read = 0u64;

    for (line_index, line) in iter.take(to_line) {

        if let Some((p, line_contents)) = parse_line_of_pid(&line) {
            if p != pid {
                continue;
            }
            stack_info.process_line(&line_contents);
            arena_info.process_line(&line_contents);
            if line_index >= from_line {
                match line_contents {
                    LineContent::LLCacheLineSwap {
                        new_start,
                        old_start,
                        size,
                        used_bytes: _,
                    } => {
                        pending_cache_line_swaps.push((new_start, old_start, size, line_index));
                    }
                    LineContent::StackForLLMiss(stack_index) => {
                        for (cache_miss_addr, evicted_addr, size, cache_miss_line_index) in
                            pending_cache_line_swaps.drain(..)
                        {
                            address_reads.add_eviction(
                                evicted_addr,
                                cache_miss_line_index,
                                stack_index,
                            );
                            address_reads.add_read(
                                cache_miss_addr,
                                cache_miss_line_index,
                                stack_index,
                            );

                            if let Some(arena_ident) =
                                arena_info.arenas().arena_covering_address(cache_miss_addr)
                            {
                                arena_reads.add_read(
                                    &arena_ident,
                                    cache_miss_addr,
                                    size,
                                    cache_miss_line_index,
                                    stack_index,
                                );
                            } else {
                                outside_arena_reads.add_read(
                                    cache_miss_addr,
                                    cache_miss_line_index,
                                    stack_index,
                                );
                                bytes_read_outside_arena += size as u64;
                            }
                            total_bytes_read += size as u64;

                            if let Some(evicted_arena_ident) =
                                arena_info.arenas().arena_covering_address(evicted_addr)
                            {
                                arena_reads.add_eviction(
                                    &evicted_arena_ident,
                                    evicted_addr,
                                    cache_miss_line_index,
                                    stack_index,
                                );
                            } else {
                                outside_arena_reads.add_eviction(
                                    evicted_addr,
                                    cache_miss_line_index,
                                    stack_index,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        };
    }

    let mut stack_table = stack_info.get_stack_table();

    println!(
        "Read {} cache-line sized memory ranges at least twice.",
        address_reads.multiple_reads_count()
    );
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

    println!(
        "Read {} ({:.0}%) bytes outside any arena.",
        bytes_read_outside_arena,
        100f64 * bytes_read_outside_arena as f64 / total_bytes_read as f64
    );
    outside_arena_reads.print_histogram();
    println!("");
    println!(
        "    Here are the top 5 cache-line sized memory ranges from outside any arena, with their reads + evictions:"
    );
    outside_arena_reads.print_top_n_stacks(5, &mut stack_table);
    println!("");
    println!("");

    let arenas_read = arena_reads.into_arenas_sorted_by_most_bytes_read();
    let mut arenas = arena_info.into_arenas();
    for (arena, (arena_address_reads, bytes)) in arenas_read.into_iter() {
        println!(
            "Read {} bytes ({:.0}%) from arena {}:",
            bytes,
            100f64 * bytes as f64 / total_bytes_read as f64,
            &arena
        );
        println!("    {}", arenas.arena_description(&arena));
        println!("");
        arena_address_reads.print_histogram();
        println!("");
        println!(
            "    Here are the top 5 cache-line sized memory ranges in this arena, with their reads + evictions:"
        );
        arena_address_reads.print_top_n_stacks(5, &mut stack_table);
        println!("");
        println!("");
    }
}
