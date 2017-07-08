use std::io;
use std::process::Command;
use nom::{digit, IResult};
use std::str::FromStr;
use std::str;

pub struct StackFrameInfo {
    pub function_name: String,
    pub file_path_str: String,
    pub line_number: usize,
}

named!(parse_one_stackframe<StackFrameInfo>, do_parse!(
    function_name: map_res!(
        take_until!(&b"\n"[..]),
        str::from_utf8
    ) >>
    tag!(b"\n") >>
    file_path_str: map_res!(
        take_until!(&b":"[..]),
        str::from_utf8
    ) >>
    tag!(b":") >>
    line_number: map_res!(
        map_res!(digit, str::from_utf8),
        FromStr::from_str
    ) >>
    tag!(b"\n") >>
    (
        StackFrameInfo {
            function_name: function_name.to_owned(),
            file_path_str: file_path_str.to_owned(),
            line_number
        }
    )
));

named!(parse_addr2line_output<&[u8], Vec<StackFrameInfo>>, many0!(parse_one_stackframe));

pub fn get_addr2line_stack(lib_path: &str, addr: u64) -> Result<Vec<StackFrameInfo>, io::Error> {
    let addr2line_output = Command::new("addr2line")
        .args(
            &[
                "--functions",
                "--demangle",
                "--inlines",
                &format!("--exe={}", lib_path),
                &format!("0x{:x}", addr),
            ],
        )
        .output()?
        .stdout;
    if let IResult::Done(_, result) = parse_addr2line_output(&addr2line_output) {
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}
