#![feature(test)]
extern crate test;
extern crate regex;
extern crate hyper;
extern crate flate2;
extern crate rayon;
#[macro_use]
extern crate nom;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate addr2line;

pub mod cache_log_parsing;
pub mod shared_libraries;
pub mod addr2line_cmd;

#[test]
fn it_works() {
    use std::io::BufReader;
    use std::fs::File;
    use cache_log_parsing::{SymbolTable};
    let reader = BufReader::new(File::open("/home/mstange/allofthelogging.txt")
                                    .unwrap());

    println!("{:?}", SymbolTable::from_breakpad_symbol_dump(reader))
}

// #[test]
// fn parse_symbol_table() {
//     // Now parsing XUL.sym.gz.
//     use std::io::BufReader;
//     use std::fs::File;
//     use flate2::read::GzDecoder;
//     use symbol_table_parsing::{SymbolTable};

//     let buf_reader = BufReader::new(GzDecoder::new(File::open("/Users/mstange/code/rust-training-crate/res/XUL.sym.gz")
//                                     .unwrap()).unwrap());
//     println!("symbol table: {:?}", SymbolTable::from_breakpad_symbol_dump(buf_reader));
// }
