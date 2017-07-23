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
                     print_multiple_read_ranges, print_cache_line_wastage,
                     print_surrounding_lines, print_wastage_source_code};

fn get_line_iter(filename: &str) -> Box<Iterator<Item = (usize, String)>> {
    let reader = BufReader::new(File::open(filename).unwrap());

    Box::new(reader.lines().enumerate().flat_map(
        |(line_index, line_result)| {
            match line_result {
                Ok(line) => Some((line_index, line)),
                Err(_) => None,
            }
        },
    ))
}

fn main() {
    let matches = clap::App::new("cache-log-parser")
        .version("0.1")
        .author("Markus Stange <mstange@themasta.com>")
        .about("Parses a log with information about memory")
        .subcommand(clap::SubCommand::with_name("list-processes")
                    .about("Lists the processes (PIDs) whose output is present in the log file.")
                    .args_from_usage(
                        "<INPUT>             'The input file to use'"))
        .subcommand(clap::SubCommand::with_name("list-sections")
                    .about("Looks for sections in the log that are marked with \"Begin ...\" and \"End ...\" and prints some information about them.")
                    .args_from_usage(
                        "-p, --pid=<PID>     'The pid of the process that should be analyzed'
                        <INPUT>              'The input file to use'"))
        .subcommand(clap::SubCommand::with_name("generate-profiles")
                    .about("Generates read_bytes, used_bytes, and wasted_bytes profiles for the given range for the given process.")
                    .args_from_usage(
                        "-p, --pid=<PID>     'The pid of the process that should be analyzed'
                        -s, --start=<START>  'The line number at which to start analyzing'
                        -e, --end=<END>      'The line number at which to stop analyzing'
                        <INPUT>              'The input file to use'"))
        .subcommand(clap::SubCommand::with_name("print-wastage-source-code")
                    .about("Prints the source code that's responsible for the most wasted bytes for the given range for the given process.")
                    .args_from_usage(
                        "-p, --pid=<PID>     'The pid of the process that should be analyzed'
                        -s, --start=<START>  'The line number at which to start analyzing'
                        -e, --end=<END>      'The line number at which to stop analyzing'
                        <INPUT>              'The input file to use'"))
        .subcommand(clap::SubCommand::with_name("analyze-double-reads")
                    .about("Checks which memory ranges are read into the cache multiple times, and prints callstacks for reads + evictions for some of them.")
                    .args_from_usage(
                        "-p, --pid=<PID>     'The pid of the process that should be analyzed'
                        -s, --start=<START>  'The line number at which to start analyzing'
                        -e, --end=<END>      'The line number at which to stop analyzing'
                        <INPUT>              'The input file to use'"))
        .subcommand(clap::SubCommand::with_name("print-context")
                    .about("Prints a small excerpt from the log, filtering out output from other processes.")
                    .args_from_usage(
                        "-p, --pid=<PID>     'The pid of the process whose log output should be printed'
                        -l, --line=<LINE>    'The line number around which to print the context'
                        -c, --context=[CONTEXT] 'How many lines of context should be printed both before and after the line in question'
                        <INPUT>              'The input file to use'"
                    ))
        .subcommand(clap::SubCommand::with_name("print-unrecognized-lines")
                    .about("Prints all lines from the log which don't match any of the known patterns.")
                    .args_from_usage(
                        "
                        <INPUT>              'The input file to use'"
                    ))
        .get_matches();

    if let Some(matches) = matches.subcommand_matches("list-processes") {
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_process_info(iter); 
    } else if let Some(matches) = matches.subcommand_matches("list-sections") {
        let pid = matches.value_of("pid").unwrap();
        let pid: i32 = pid.parse().expect("pid needs to be an integer");
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_display_list_info(pid, iter);
    } else if let Some(matches) = matches.subcommand_matches("print-context") {
        let pid = matches.value_of("pid").unwrap();
        let pid: i32 = pid.parse().expect("pid needs to be an integer");
        let line_index = matches.value_of("line").unwrap();
        let line_index: usize = line_index.parse().expect("line number needs to be an unsigned integer");
        let context = matches.value_of("context").unwrap_or("").parse().unwrap_or(12);
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_surrounding_lines(pid, iter, line_index, context);
    } else if let Some(matches) = matches.subcommand_matches("generate-profiles") {
        let pid = matches.value_of("pid").unwrap();
        let pid: i32 = pid.parse().expect("pid needs to be an integer");
        let start_line_index = matches.value_of("start").unwrap();
        let start_line_index: usize = start_line_index.parse().expect("start line number needs to be an unsigned integer");
        let end_line_index = matches.value_of("end").unwrap();
        let end_line_index: usize = end_line_index.parse().expect("end line number needs to be an unsigned integer");
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_cache_line_wastage(pid, iter, start_line_index, end_line_index);
    } else if let Some(matches) = matches.subcommand_matches("print-wastage-source-code") {
        let pid = matches.value_of("pid").unwrap();
        let pid: i32 = pid.parse().expect("pid needs to be an integer");
        let start_line_index = matches.value_of("start").unwrap();
        let start_line_index: usize = start_line_index.parse().expect("start line number needs to be an unsigned integer");
        let end_line_index = matches.value_of("end").unwrap();
        let end_line_index: usize = end_line_index.parse().expect("end line number needs to be an unsigned integer");
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_wastage_source_code(pid, iter, start_line_index, end_line_index);
    } else if let Some(matches) = matches.subcommand_matches("analyze-double-reads") {
        let pid = matches.value_of("pid").unwrap();
        let pid: i32 = pid.parse().expect("pid needs to be an integer");
        let start_line_index = matches.value_of("start").unwrap();
        let start_line_index: usize = start_line_index.parse().expect("start line number needs to be an unsigned integer");
        let end_line_index = matches.value_of("end").unwrap();
        let end_line_index: usize = end_line_index.parse().expect("end line number needs to be an unsigned integer");
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_multiple_read_ranges(pid, iter, start_line_index, end_line_index);
    } else if let Some(matches) = matches.subcommand_matches("print-unrecognized-lines") {
        let iter = get_line_iter(matches.value_of("INPUT").unwrap());
        print_other_lines(iter);
    }

    // let result = 
    // // print_process_info(iter);
    // print_display_list_info(31480, iter);
    // // print_cache_contents_at(iter, 67317106)?;
    // // print_cache_contents_at(iter, cache, 58256278)?;
    // // print_other_lines(iter);
    // // print_multiple_read_ranges(iter, 112906438, 113242142);
    // // print_surrounding_lines(31480, iter, 174117724);
    // println!("result: {:?}", result);
}
