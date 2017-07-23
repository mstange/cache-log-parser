use std::collections::{HashMap, HashSet};
use addr2line_cmd::{get_addr2line_stack, get_addr2line_symbols_with_inline, StackFrameInfo};
use shared_libraries::SharedLibraries;
use std::iter;

#[derive(Clone)]
pub struct StackEntry {
    pub parent_stack: usize,
    pub frame: usize,
}

pub struct StackTableConverter<'a> {
    old_frame_to_new_frame: HashMap<usize, usize>,
    old_stack_to_new_stack: HashMap<usize, usize>,
    stack_table: &'a StackTable,
    new_stack_table: StackTable,
}

impl<'b> StackTableConverter<'b> {
    pub fn new(stack_table: &StackTable) -> StackTableConverter {
        let mut new_stack_table = StackTable::new();
        if let Some(libs) = stack_table.libs.clone() {
            new_stack_table.set_libs(libs);
        }

        StackTableConverter {
            old_frame_to_new_frame: HashMap::new(),
            old_stack_to_new_stack: HashMap::new(),
            stack_table,
            new_stack_table,
        }
    }

    pub fn convert_frame(&mut self, frame: usize) -> usize {
        if let Some(new_frame) = self.old_frame_to_new_frame.get(&frame) {
            return *new_frame;
        }

        let new_frame = self.new_stack_table.frames.len();
        let (addr, _) = self.stack_table.frames[frame];
        self.new_stack_table.add_frame(new_frame, addr);
        self.old_frame_to_new_frame.insert(frame, new_frame);
        new_frame
    }

    pub fn convert_stack(&mut self, stack: usize) -> usize {
        // println!("convert_stack({})", stack);
        if let Some(new_stack) = self.old_stack_to_new_stack.get(&stack) {
            // println!("have the stack {}", stack);
            return *new_stack;
        }

        let StackEntry {
            parent_stack,
            frame,
        } = self.stack_table.stacks[stack].clone();
        let new_parent_stack = if stack == 0 && parent_stack == 0 {
            0
        } else {
            self.convert_stack(parent_stack)
        };
        let new_frame = self.convert_frame(frame);
        let new_stack = self.new_stack_table.stacks.len();
        self.new_stack_table.add_stack(
            new_stack,
            new_parent_stack,
            new_frame,
        );
        self.old_stack_to_new_stack.insert(stack, new_stack);
        new_stack
    }

    pub fn into_new_stack_table(self) -> (StackTable, HashMap<usize, usize>) {
        (self.new_stack_table, self.old_stack_to_new_stack)
    }
}

#[derive(Clone)]
pub struct StackTable {
    pub frames: Vec<(u64, Option<Vec<StackFrameInfo>>)>,
    pub stacks: Vec<StackEntry>,
    pub libs: Option<SharedLibraries>,
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
        assert_eq!(index, self.frames.len(), "unexpected frame index");
        self.frames.push((address, None));
    }

    pub fn add_stack(&mut self, index: usize, parent_stack: usize, frame: usize) {
        assert_eq!(index, self.stacks.len(), "unexpected stack index");
        assert!(
            parent_stack < index || parent_stack == 0,
            "can't refer to parent stacks that I haven't seen yet"
        );
        assert!(
            frame < self.frames.len(),
            "can't refer to frames that I haven't seen yet"
        );
        self.stacks.push(StackEntry {
            parent_stack,
            frame,
        });
    }

    pub fn create_reduced_table_containing_stacks(
        &self,
        stacks: &HashSet<usize>,
    ) -> (StackTable, HashMap<usize, usize>) {
        println!("creating reduced stack table with {} stacks.", stacks.len());
        let mut converter = StackTableConverter::new(self);
        for stack in stacks {
            // println!("converting stack {}", *stack);
            converter.convert_stack(*stack);
        }
        converter.into_new_stack_table()
    }

    pub fn resolve_inline_symbols(&mut self) -> Vec<usize> {
        // Every frame that has multiple StackFrameInfos will now have multiple frames,
        // and every stack that uses such a frame needs to be changed to multiple stacks
        // and all references to that stack need to be changed to point to the leaf stack.
        let mut old_frame_to_additional_frames: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut frames_to_add: Vec<(u64, Option<Vec<StackFrameInfo>>)> = Vec::new();
        let old_frames_len = self.frames.len();
        for (frame, &mut (address, ref mut frame_infos)) in self.frames.iter_mut().enumerate() {
            if let &mut Some(ref mut frame_infos) = frame_infos {
                if frame_infos.len() > 1 {
                    let mut additional_frames_for_this_frame = Vec::new();
                    for frame_info in frame_infos.drain(1..) {
                        additional_frames_for_this_frame.push(old_frames_len + frames_to_add.len());
                        frames_to_add.push((address, Some(vec![frame_info])))
                    }
                    old_frame_to_additional_frames.insert(frame, additional_frames_for_this_frame);
                }
            }
        }
        self.frames.extend(frames_to_add);

        let mut new_stacks = Vec::with_capacity(self.stacks.len());
        new_stacks.push(StackEntry {
            parent_stack: 0,
            frame: 0
        });
        let mut old_stack_to_new_stack = Vec::with_capacity(self.stacks.len());
        old_stack_to_new_stack.push(0);
        for &StackEntry{ parent_stack, frame } in self.stacks.iter().skip(1) {
            new_stacks.push(StackEntry{
                parent_stack: old_stack_to_new_stack[parent_stack],
                frame
            });
            if let Some(additional_frames) = old_frame_to_additional_frames.get(&frame) {
                for additional_frame in additional_frames {
                    let parent_stack = new_stacks.len() - 1;
                    new_stacks.push(StackEntry{
                        parent_stack,
                        frame: *additional_frame,
                    });
                }
            }
            old_stack_to_new_stack.push(new_stacks.len() - 1);
        }

        self.stacks = new_stacks;
        old_stack_to_new_stack
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
        result
    }

    pub fn symbolicate_frames<T>(&mut self, frames: T)
        where T: iter::Iterator<Item = usize> {
        if let &Some(ref libs) = &self.libs {
            let mut frames_by_lib_index = HashMap::new();
            for frame in frames {
                let (address, _) = self.frames[frame];
                if let Some(lib) = libs.lib_for_address(address) {
                    frames_by_lib_index
                        .entry(lib)
                        .or_insert_with(|| Vec::new())
                        .push((frame, address - lib.start));
                }
            }
            for (lib, frames_with_addresses) in frames_by_lib_index.into_iter() {
                if let Ok(symbolicated_addresses) =
                    get_addr2line_symbols_with_inline(
                        &lib.debug_path,
                        &frames_with_addresses
                            .iter()
                            .map(|&(_, address)| address)
                            .collect(),
                    )
                {
                    for (i, frame_info) in symbolicated_addresses.into_iter().enumerate() {
                        let (frame, _) = frames_with_addresses[i];
                        self.frames[frame].1 = Some(frame_info);
                    }
                }
            }
        }

    }

    pub fn symbolicate_all(&mut self) {
        println!("Have {} frames I need to symbolicate.", self.frames.len());
        let frame_count = self.frames.len();
        self.symbolicate_frames(0..frame_count);
    }

    pub fn print_stack(&mut self, stack: usize, indent: usize) {
        for frame in self.frame_index_list_for_stack(stack) {
            let &mut (address, ref mut stack_frame_info) = &mut self.frames[frame];
            if let &Some(ref libs) = &self.libs {
                if let Some(lib) = libs.lib_for_address(address) {
                    let relative_address = address - lib.start;

                    if let None = *stack_frame_info {
                        if let Ok(mut stack_fragment) =
                            get_addr2line_stack(&lib.debug_path, relative_address)
                        {
                            stack_fragment.reverse();
                            *stack_frame_info = Some(stack_fragment);
                        }
                    }

                    if let &mut Some(ref stack_fragment) = stack_frame_info {
                        for &StackFrameInfo {
                            ref function_name,
                            ref file_path_str,
                            ref line_number,
                        } in stack_fragment
                        {
                            println!(
                                "{e:indent$}{} ({}:{})",
                                function_name,
                                file_path_str,
                                line_number,
                                e = "",
                                indent = indent
                            );
                        }
                    } else {
                        println!(
                            "{e:indent$}0x{:016x} [{} + 0x{:x}]",
                            address,
                            lib.name,
                            relative_address,
                            e = "",
                            indent = indent
                        );
                    }
                } else {
                    println!(
                        "{e:indent$}0x{:016x} [unknown binary]",
                        address,
                        e = "",
                        indent = indent
                    );
                }
            } else {
                println!("{e:indent$}0x{:016x}", address, e = "", indent = indent);
            }
        }
    }

    pub fn set_libs(&mut self, libs: SharedLibraries) {
        self.libs = Some(libs);
    }
}
