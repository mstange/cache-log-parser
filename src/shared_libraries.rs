use serde_json;
use std::cmp::Ordering;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedLibrary {
    pub start: u64,
    pub end: u64,
    pub offset: u64,
    pub name: String,
    pub path: String,
    pub debug_name: String,
    pub debug_path: String,
    pub breakpad_id: String,
    pub arch: String,
}

pub struct SharedLibraries {
    l: Vec<SharedLibrary>,
}

impl SharedLibraries {
    pub fn new() -> SharedLibraries {
        SharedLibraries { l: Vec::new() }
    }

    pub fn from_json_string(json_string: String) -> serde_json::Result<SharedLibraries> {
        Ok(SharedLibraries { l: serde_json::from_str(&json_string)? })
    }

    pub fn lib_for_address<'a>(&'a self, addr: u64) -> Option<&'a SharedLibrary> {
        if let Ok(index) = self.l
               .binary_search_by(|ref lib| {
            // Return a statement about lib. (Is lib less / equal / greater than addr?)
            if lib.start <= addr {
                if addr < lib.end {
                    Ordering::Equal
                } else {
                    Ordering::Less
                }
            } else {
                Ordering::Greater
            }
        }) {
            Some(&self.l[index])
        } else {
            None
        }
    }
}
