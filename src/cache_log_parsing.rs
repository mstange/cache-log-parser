use std::num::ParseIntError;
use std::result;
use nom::{hex_digit, digit, rest_s, IResult};
use std::str::FromStr;

#[derive(Debug, PartialEq)]
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

named!(parse_line_of_pid_impl<&str, (i32, LineContent)>, do_parse!(
    tag!("==") >>
    pid: map_res!(digit, FromStr::from_str) >>
    tag!("== ") >>
    line_content: parse_line >>
    ((pid, line_content))
));

pub fn parse_line_of_pid(line: &str) -> Option<(i32, LineContent)> {
    match parse_line_of_pid_impl(line) {
        IResult::Done(_, val) => Some(val),
        _ => None
    }
}

#[test]
fn test_parse_line() {
    assert_eq!(
        parse_line("LLCacheSwap: new_start=1ffefffe00 old_start=0 size=64"),
        IResult::Done(
            "",
            LineContent::LLCacheLineSwap {
                new_start: 0x1ffefffe00,
                old_start: 0x0,
                size: 64,
            },
        )
    );
    assert_eq!(
        parse_line("LLMiss: why=I1_NoX size=3 addr=000000000596e8fe tid=1"),
        IResult::Done(
            "",
            LineContent::LLMiss {
                why: "I1_NoX",
                size: 3,
                addr: 0x596e8fe,
                tid: 1,
            },
        )
    );

    assert_eq!(
        parse_line_of_pid(
            "==16935== LLCacheSwap: new_start=1ffeffe400 old_start=0 size=64",
        ),
        IResult::Done("", (
            16935,
            LineContent::LLCacheLineSwap {
                new_start: 0x1ffeffe400,
                old_start: 0x0,
                size: 64,
            },
        ))
    );
}
