use addr2line_cmd::{get_addr2line_stack, StackFrameInfo};
use shared_libraries::SharedLibraries;

struct StackEntry {
    parent_stack: usize,
    frame: usize,
}

pub struct StackTable {
    frames: Vec<(u64, Option<Vec<StackFrameInfo>>)>,
    stacks: Vec<StackEntry>,
    libs: Option<SharedLibraries>,
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
        self.frames.push((address, None));
    }

    pub fn add_stack(&mut self, index: usize, parent_stack: usize, frame: usize) {
        assert!(index == self.stacks.len(), "unexpected stack index");
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

    pub fn print_stack(&mut self, stack: usize) {
        for frame in self.frame_index_list_for_stack(stack) {
            let &mut (address, ref mut stack_frame_info) = &mut self.frames[frame];
            if let &Some(ref libs) = &self.libs {
                if let Some(lib) = libs.lib_for_address(address) {
                    let relative_address = address - lib.start;

                    if let None = *stack_frame_info {
                        if let Ok(stack_fragment) =
                            get_addr2line_stack(&lib.debug_path, relative_address)
                        {
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
                            println!("  {} ({}:{})", function_name, file_path_str, line_number);
                        }
                    } else {
                        println!(
                            "  0x{:016x} [{} + 0x{:x}]",
                            address,
                            lib.name,
                            relative_address
                        );
                    }
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
