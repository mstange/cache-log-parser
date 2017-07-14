use std::collections::HashSet;
use stack_table::{StackTable, StackEntry};
use addr2line_cmd::{StackFrameInfo, get_addr2line_symbols_no_inline};
use serde_json::{Value, Number, to_writer};
use std::io;
use std::fs::File;

pub struct ProfileBuilder {
    stack_table: StackTable,
    samples: Vec<(usize, f64)>,
    used_stacks: HashSet<usize>,
    interval: f64,
}

impl ProfileBuilder {
    pub fn new(stack_table: StackTable, interval: f64) -> ProfileBuilder {
        ProfileBuilder {
            stack_table,
            samples: Vec::new(),
            used_stacks: HashSet::new(),
            interval,
        }
    }

    pub fn add_sample(&mut self, stack: usize, time: f64) {
        self.samples.push((stack, time));
        self.used_stacks.insert(stack);
    }

    pub fn save_to_file(&mut self, filename: &str) -> Result<(), io::Error> {
        println!("Have {} samples.", self.samples.len());
        let (mut stack_table, old_stack_to_new_stack) =
            self.stack_table.create_reduced_table_containing_stacks(
                &self.used_stacks,
            );
        println!("Symbolicating...");
        stack_table.symbolicate_all();
        println!("Done symbolicating.");
        let non_inline_to_inline_stack = stack_table.resolve_inline_symbols();
        let frame_table_data: Vec<Value> = stack_table
            .frames
            .iter()
            .enumerate()
            .map(|(frame, _)| json!([frame]))
            .collect();
        let stack_table_data: Vec<Value> = stack_table
            .stacks
            .iter()
            .enumerate()
            .map(|(stack,
              &StackEntry {
                  parent_stack,
                  frame,
              })| {
                if stack == 0 && parent_stack == 0 {
                    json!([null, frame])
                } else {
                    json!([parent_stack, frame])
                }
            })
            .collect();
        let samples_data: Vec<Value> = self.samples
            .iter()
            .enumerate()
            .map(|(i, &(stack, time))| {
                json!(
                    [
                        non_inline_to_inline_stack[*old_stack_to_new_stack.get(&stack).expect(
                            "Found untranslated stack",
                        )],
                        time,
                        0,
                    ]
                )
            })
            .collect();
        let string_table: Vec<Value> = stack_table
            .frames
            .iter()
            .enumerate()
            .map(|(_, &(address, ref frame_info_vec))| {
                if let &Some(ref frame_info_vec) = frame_info_vec {
                    if !frame_info_vec.is_empty() {
                        let &StackFrameInfo {
                            ref function_name,
                            ref file_path_str,
                            line_number,
                        } = &frame_info_vec[0];
                        return Value::String(format!(
                            "{} ({}:{})",
                            function_name,
                            file_path_str,
                            line_number
                        ));
                    }
                }
                Value::String(format!("0x{:x}", address))

            })
            .collect();
        let profile = json!({
            "meta": {
                "version": 4,
                "processType": 0,
                "interval": self.interval
            },
            "libs": [],
            "threads": [
                {
                    "name": "All",
                    "processType": "default",
                    "frameTable": {
                        "schema": {
                            "location": 0,
                            "implementation": 1,
                            "optimizations": 2,
                            "line": 3,
                            "category": 4
                        },
                        "data": frame_table_data
                    },
                    "stackTable": {
                        "schema": {
                            "prefix": 0,
                            "frame": 1,
                        },
                        "data": stack_table_data
                    },
                    "samples": {
                        "schema": {
                            "stack": 0,
                            "time": 1,
                            "responsiveness": 2,
                            "rss": 3,
                            "uss": 4
                        },
                        "data": samples_data
                    },
                    "markers": {
                        "schema": {
                            "name": 0,
                            "time": 1,
                            "data": 2
                        },
                        "data": []
                    },
                    "stringTable": string_table
                }
            ]
        });
        let file = File::create(filename)?;
        to_writer(file, &profile).expect("Couldn't write JSON");
        Ok(())
    }
}
