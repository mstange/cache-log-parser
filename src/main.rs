#![feature(test)]
extern crate test;
extern crate regex;
extern crate hyper;
extern crate flate2;
extern crate rayon;
#[macro_use]
extern crate nom;
extern crate serde;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate clap;
extern crate rand;
extern crate itertools;
extern crate pretty_bytes;
extern crate fixed_circular_buffer;

mod cache_log_parsing;
mod shared_libraries;
mod addr2line_cmd;
mod ranges;
mod cpucache;
mod stack_table;
mod cache_log_info;
mod arenas;
mod profile;

use std::io::{BufRead, BufReader};
use std::fs::File;
use cache_log_info::{print_display_list_info, print_other_lines, print_process_info,
                     print_extra_field_info, print_multiple_read_ranges, print_cache_line_wastage,
                     print_fork_lines, print_surrounding_lines};

fn main() {
    let matches = clap::App::new("cache-log-parser")
        .version("0.1")
        .author("Markus Stange <mstange@themasta.com>")
        .about("Parses a log with information about memory")
        .args_from_usage("<INPUT>              'Sets the input file to use'")
        .get_matches();

    let reader = BufReader::new(File::open(matches.value_of("INPUT").unwrap()).unwrap());

    let iter = reader.lines().enumerate().flat_map(
        |(line_index, line_result)| {
            match line_result {
                Ok(line) => Some((line_index, line)),
                Err(_) => None,
            }
        },
    );
    let result = 
    // print_process_info(iter);
    // print_display_list_info(31480, iter);
    // print_cache_contents_at(iter, 67317106)?;
    // print_cache_contents_at(iter, cache, 58256278)?;
    // print_extra_field_info(iter).expect("someting wong");
    // print_other_lines(iter);
    // print_fork_lines(31480, iter);
    // print_cache_read_overhead(iter, 67317106, 67500260)?;
    // print_multiple_read_ranges(iter, 112906438, 113242142);
    print_cache_line_wastage(31480, iter, 132184857, 132777404);
    // print_surrounding_lines(31480, iter, 174117724);
    println!("result: {:?}", result);
}
