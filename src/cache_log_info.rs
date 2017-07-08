use std::io;
use std::iter;
use std::collections::HashMap;
use std::fmt::Display;
use cache_log_parsing::{parse_line_of_pid, LineContent};
use ranges::Ranges;
use cpucache::CPUCache;
use stack_table::StackTable;
use shared_libraries::SharedLibraries;

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

#[allow(dead_code)]
pub fn print_multiple_read_ranges<T>(iter: iter::Enumerate<io::Lines<T>>,
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

        if let Some((_, line_contents)) = parse_line_of_pid(&line?) {
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
