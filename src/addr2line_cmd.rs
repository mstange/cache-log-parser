use std::io;
use std::process::Command;
use nom::{digit, IResult};
use std::str::FromStr;
use std::str;
use itertools::Itertools;

#[derive(Debug,Clone,PartialEq,Eq,Hash)]
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
    line_number: opt!(map_res!(
        map_res!(digit, str::from_utf8),
        FromStr::from_str
    )) >>
    opt!(take_until!(&b"\n"[..])) >>
    tag!(b"\n") >>
    (
        StackFrameInfo {
            function_name: function_name.to_owned(),
            file_path_str: file_path_str.to_owned(),
            line_number: line_number.unwrap_or(0),
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
    if let IResult::Done(_, mut result) = parse_addr2line_output(&addr2line_output) {
        result.reverse();
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}

pub fn get_addr2line_symbols_with_inline(
    lib_path: &str,
    addrs: &Vec<u64>,
) -> Result<Vec<Vec<StackFrameInfo>>, io::Error> {
    let addrs_as_strings: Vec<String> = addrs
        .iter()
        .map(|addr| format!("0x{:x}", addr))
        .intersperse("0".to_owned())
        .collect();
    let addr2line_output = Command::new("addr2line")
        .args(
            [
                "--inlines",
                "--functions",
                "--demangle",
                &format!("--exe={}", lib_path),
            ].iter(),
        )
        .args(&addrs_as_strings)
        .output()?
        .stdout;
    if let IResult::Done(_, flat_frame_infos) = parse_addr2line_output(&addr2line_output) {
        let mut result = Vec::new();
        let mut iter = flat_frame_infos.into_iter();
        'outer: while let Some(frame_info) = iter.next() {
            let mut this_address_frames = vec![frame_info];
            while let Some(frame_info) = iter.next() {
                if frame_info.function_name != "??" {
                    this_address_frames.push(frame_info);
                } else {
                    this_address_frames.reverse();
                    result.push(this_address_frames);
                    continue 'outer;
                }
            }
            this_address_frames.reverse();
            result.push(this_address_frames);
            break;
        }
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}


#[test]
fn test_inlined_frames() {
    let _ = get_addr2line_symbols_with_inline(
        "/home/mstange/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/libxul.so",
        &vec![0x161430a, 0x161e2cb, 0x446f710, 0x446f2b5, 0x446de31, 0x4471c1c, 0xb1264a, 0xb13145, 0x1664c89, 0x490daa2, 0x4654958, 0x465454f, 0x4653e74, 0x464eb57, 0x46545ae, 0x49dfb51, 0x49df9fc, 0x49d6ac9, 0x49e4f64, 0x46542d4, 0x49820d2, 0x1666e89, 0x1661e4f, 0xb12475, 0x16190ef, 0x16062b6, 0x1605036, 0x1603355, 0x48e4e7a, 0x48da88d, 0x4656202, 0x4656111, 0x1666679, 0x163d5ea, 0x163cedf, 0x48e07eb, 0x48e0614, 0x4aa4fea, 0x1670478, 0x4653ed2, 0x46d4a32, 0x46d270c, 0x4695970, 0x4695b40, 0x496097e, 0x49548a6, 0x4b556b8, 0x4b554d0, 0x4695bc4, 0x4965c6a, 0x49db595, 0x49d6088, 0x49407be, 0x493fdb1, 0x493fc7f, 0x494b5fb, 0x494b46d, 0x494a0cc, 0x46da6d6, 0x4cb95a0, 0x46ec0fa, 0x46da333, 0x46edd00, 0x4cb954c, 0x484d9b2, 0x46d2725, 0x4afcd84, 0x494a044, 0x4954938, 0x4b53225, 0x4b51cb8, 0x46da53b, 0x4851b49, 0x4960c0d, 0x4b55771, 0x4a9c7a0, 0x4954850, 0x464773c, 0x463fa7e, 0x4aa4c6a, 0x4a98609, 0x4654a3f, 0x166712d, 0x16623a3, 0x163fa08, 0x163f1fd, 0x166da7c, 0x1654ae0, 0x48debd8, 0x4c4451a, 0x496107c, 0x16704c2, 0x163b5c1, 0x164f3dc, 0x164f27d, 0x166df4f, 0xa8de3e, 0x1615003, 0x49db520, 0x163ce02, 0x48de28f, 0x49608c7, 0x161470a, 0x4cb98ce, 0x473fdc2, 0x46edcc3, 0x4b1ef10, 0x1666677, 0x163d2ad, 0x163b852, 0x164f324, 0x16151d1, 0x4cb98cf, 0x16664f0, 0x163d1e7, 0x163b7e0, 0x162f486, 0x4644e42, 0x465ad9e, 0x498ab8f, 0x4983dbd, 0x4962528, 0x4aaa660, 0x4abb7ec, 0x494a0a6, 0x46da371, 0x46d4dc5, 0x49d60ab, 0x49d88ff, 0x4965c28, 0x4ab1c27, 0x4ae0596, 0x4af2851, 0x4af248a, 0x4afce04, 0x162f4d1, 0xaeb23e, 0x4695a21, 0x4960589, 0x46d276d, 0x464704f, 0x163b52c, 0xb0fbea, 0x166db12, 0x166951b, 0x1668eba, 0x4737b3f, 0x47510c5, 0x163b54c, 0x162f0c2, 0x4cb9724, 0x474020f, 0x162f0d1, 0x48debf1, 0x4961340, 0x495613a, 0x498e603, 0x494b469, 0x46d9f80, 0x46d9f7c, 0x4cb9770, 0x4851acf, 0x166dc2f, 0x166b0ee, 0x166a966, 0x163b528, 0x4653e8a, 0x4b01b09, 0x166d803, 0x1614632, 0xb0fbee, 0x163b575, 0x46d9e8a, 0x163b600, 0x48df5de, 0x48daeb2, 0x49660ec, 0x4965f5b, 0x4ab290a, 0x4b24de9, 0x4b50ab4, 0x47346b8, 0x4b24959, 0x4b24955, 0x163b891, 0x4af27d0, 0x463fa02, 0x49e52d8, 0x49d667a, 0x46cfa7f, 0x46cfa7b, 0x1661116, 0x46ec10f, 0x1661110, 0x4b24eb4, 0x4b1b2e1, 0x4b1b12b, 0x47a6f94, 0x163cf2e, 0x164ce72, 0x1669581, 0x1657595, 0x4931967, 0x493183d, 0x4aaa364, 0x4aaa0ba, 0x4a9da19, 0x163d43a, 0x1614342, 0x4c9d315, 0x4a4d898, 0x4cb956c, 0x46ff99d, 0x4a4d8bd, 0x4a4b13e, 0x4659abb, 0x162f0d3, 0xb11d40, 0x4cb98ad, 0x166a96b, 0xb11e30, 0x16549fb, 0x161e2b3],
    );
}