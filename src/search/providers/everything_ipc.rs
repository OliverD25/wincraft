//! The messages Everything (voidtools) 1.4 understands, written out from its
//! public `everything_ipc.h`, with no SDK DLL. A query goes to Everything's
//! window as `WM_COPYDATA`; the answer comes back the same way, to a window
//! of ours, as a list of items with the fields that were asked for.
//!
//! Everything here is plain data and byte handling, so the layouts and the
//! parsing are tested without Everything running.

/// The window class of a running Everything 1.4 (and 1.3).
pub const WINDOW_CLASS: &str = "EVERYTHING_TASKBAR_NOTIFICATION";
/// Everything 1.5 alpha: a different window, and an IPC this code does not
/// speak.
pub const WINDOW_CLASS_15A: &str = "EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)";

/// `dwData` of the `WM_COPYDATA` that carries a QUERY2 with a UTF-16 string.
pub const COPYDATA_QUERY2W: usize = 18;

/// `wParam` values of a plain `SendMessage(window, WM_USER, ...)`.
pub const GET_MAJOR_VERSION: usize = 0;
pub const GET_MINOR_VERSION: usize = 1;
pub const IS_DB_LOADED: usize = 401;

/// QUERY2 request flags: which fields each item of the answer carries. They
/// come back in this order, lowest bit first.
pub const REQUEST_NAME: u32 = 0x0000_0001;
pub const REQUEST_PATH: u32 = 0x0000_0002;
pub const REQUEST_EXTENSION: u32 = 0x0000_0008;
pub const REQUEST_SIZE: u32 = 0x0000_0010;
pub const REQUEST_DATE_CREATED: u32 = 0x0000_0020;
pub const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
pub const REQUEST_DATE_ACCESSED: u32 = 0x0000_0080;
pub const REQUEST_ATTRIBUTES: u32 = 0x0000_0100;

/// Item flags.
pub const ITEM_FOLDER: u32 = 1;
pub const ITEM_DRIVE: u32 = 2;
pub const ITEM_ROOT: u32 = 4;

/// Sort types.
pub const SORT_NAME_ASCENDING: u32 = 1;

/// What a query asks for. The fields are the ones this code reads.
pub const REQUEST_FIELDS: u32 = REQUEST_NAME | REQUEST_PATH | REQUEST_SIZE | REQUEST_DATE_MODIFIED;

/// `EVERYTHING_IPC_QUERY2`, followed in the message by the search text as
/// UTF-16 with its closing NUL.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Query2Header {
    /// The window the answer is sent to, as the 32 bits of its handle.
    pub reply_hwnd: u32,
    /// The `dwData` the answer carries, so it can be matched to its query.
    pub reply_copydata_message: u32,
    pub search_flags: u32,
    pub offset: u32,
    pub max_results: u32,
    pub request_flags: u32,
    pub sort_type: u32,
}

/// `EVERYTHING_IPC_LIST2`, followed by one `Item2` per item.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct List2Header {
    /// How many items match in all.
    pub total_items: u32,
    /// How many are in this answer.
    pub num_items: u32,
    pub offset: u32,
    pub request_flags: u32,
    pub sort_type: u32,
}

/// `EVERYTHING_IPC_ITEM2`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Item2 {
    pub flags: u32,
    /// Where this item's fields start, counted from the start of the list.
    pub data_offset: u32,
}

pub const QUERY2_HEADER_SIZE: usize = std::mem::size_of::<Query2Header>();
pub const LIST2_HEADER_SIZE: usize = std::mem::size_of::<List2Header>();
pub const ITEM2_SIZE: usize = std::mem::size_of::<Item2>();

/// The `WM_COPYDATA` payload for a query: the header, then the text.
pub fn build_query(header: &Query2Header, text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(QUERY2_HEADER_SIZE + (text.len() + 1) * 2);
    for field in [
        header.reply_hwnd,
        header.reply_copydata_message,
        header.search_flags,
        header.offset,
        header.max_results,
        header.request_flags,
        header.sort_type,
    ] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    for unit in text.encode_utf16().chain(std::iter::once(0)) {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// One match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub name: String,
    /// The folder that holds it; empty for a drive.
    pub path: String,
    pub folder: bool,
    pub drive: bool,
    pub size: Option<u64>,
    /// Windows FILETIME of the last change.
    pub modified: Option<u64>,
}

impl Hit {
    /// The full path; a drive is `D:\`.
    pub fn full_path(&self) -> String {
        if self.path.is_empty() {
            if self.name.ends_with('\\') {
                self.name.clone()
            } else {
                format!("{}\\", self.name)
            }
        } else {
            format!("{}\\{}", self.path.trim_end_matches('\\'), self.name)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub total: u32,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Shorter than its own header, or an item or field runs past the end.
    Truncated,
    /// The answer carries a field this code cannot read.
    UnsupportedFields(u32),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Truncated => f.write_str("the answer is cut short"),
            ParseError::UnsupportedFields(flags) => {
                write!(
                    f,
                    "the answer has fields this code cannot read ({flags:#x})"
                )
            }
        }
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ParseError> {
    let slice = bytes.get(at..at + 4).ok_or(ParseError::Truncated)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn u64_at(bytes: &[u8], at: usize) -> Result<u64, ParseError> {
    let slice = bytes.get(at..at + 8).ok_or(ParseError::Truncated)?;
    let mut array = [0u8; 8];
    array.copy_from_slice(slice);
    Ok(u64::from_le_bytes(array))
}

/// A string field: its length in UTF-16 units, then the units, then a NUL.
/// Returns the text and the position after the field.
fn string_at(bytes: &[u8], at: usize) -> Result<(String, usize), ParseError> {
    let length = u32_at(bytes, at)? as usize;
    let start = at + 4;
    let end = length
        .checked_mul(2)
        .and_then(|size| start.checked_add(size))
        .ok_or(ParseError::Truncated)?;
    // The closing NUL must be there too.
    if bytes.len() < end + 2 {
        return Err(ParseError::Truncated);
    }
    let units: Vec<u16> = bytes[start..end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Ok((String::from_utf16_lossy(&units), end + 2))
}

/// Reads an answer made with `REQUEST_FIELDS` or any subset of the fields
/// this code knows (name, path, extension, size, the three dates, attributes).
pub fn parse_list2(bytes: &[u8]) -> Result<Reply, ParseError> {
    if bytes.len() < LIST2_HEADER_SIZE {
        return Err(ParseError::Truncated);
    }
    let header = List2Header {
        total_items: u32_at(bytes, 0)?,
        num_items: u32_at(bytes, 4)?,
        offset: u32_at(bytes, 8)?,
        request_flags: u32_at(bytes, 12)?,
        sort_type: u32_at(bytes, 16)?,
    };
    let known = REQUEST_NAME
        | REQUEST_PATH
        | REQUEST_EXTENSION
        | REQUEST_SIZE
        | REQUEST_DATE_CREATED
        | REQUEST_DATE_MODIFIED
        | REQUEST_DATE_ACCESSED
        | REQUEST_ATTRIBUTES;
    if header.request_flags & !known != 0 {
        return Err(ParseError::UnsupportedFields(header.request_flags & !known));
    }
    let count = header.num_items as usize;
    let items_end = count
        .checked_mul(ITEM2_SIZE)
        .and_then(|size| size.checked_add(LIST2_HEADER_SIZE))
        .ok_or(ParseError::Truncated)?;
    if bytes.len() < items_end {
        return Err(ParseError::Truncated);
    }
    let mut hits = Vec::with_capacity(count);
    for index in 0..count {
        let at = LIST2_HEADER_SIZE + index * ITEM2_SIZE;
        let flags = u32_at(bytes, at)?;
        let mut cursor = u32_at(bytes, at + 4)? as usize;
        let mut hit = Hit {
            name: String::new(),
            path: String::new(),
            folder: flags & ITEM_FOLDER != 0,
            drive: flags & (ITEM_DRIVE | ITEM_ROOT) != 0,
            size: None,
            modified: None,
        };
        let wanted = header.request_flags;
        if wanted & REQUEST_NAME != 0 {
            let (text, next) = string_at(bytes, cursor)?;
            hit.name = text;
            cursor = next;
        }
        if wanted & REQUEST_PATH != 0 {
            let (text, next) = string_at(bytes, cursor)?;
            hit.path = text;
            cursor = next;
        }
        if wanted & REQUEST_EXTENSION != 0 {
            cursor = string_at(bytes, cursor)?.1;
        }
        if wanted & REQUEST_SIZE != 0 {
            // A folder's size can be -1 when Everything does not index it.
            let size = u64_at(bytes, cursor)? as i64;
            hit.size = u64::try_from(size).ok();
            cursor += 8;
        }
        if wanted & REQUEST_DATE_CREATED != 0 {
            u64_at(bytes, cursor)?;
            cursor += 8;
        }
        if wanted & REQUEST_DATE_MODIFIED != 0 {
            let time = u64_at(bytes, cursor)?;
            hit.modified = (time != u64::MAX && time != 0).then_some(time);
            cursor += 8;
        }
        if wanted & REQUEST_DATE_ACCESSED != 0 {
            u64_at(bytes, cursor)?;
            cursor += 8;
        }
        if wanted & REQUEST_ATTRIBUTES != 0 {
            u32_at(bytes, cursor)?;
        }
        hits.push(hit);
    }
    Ok(Reply {
        total: header.total_items,
        hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn the_structs_have_the_layouts_of_everything_ipc_h() {
        // EVERYTHING_IPC_QUERY2: seven DWORDs, the text right after.
        assert_eq!(size_of::<Query2Header>(), 28);
        assert_eq!(offset_of!(Query2Header, reply_hwnd), 0);
        assert_eq!(offset_of!(Query2Header, reply_copydata_message), 4);
        assert_eq!(offset_of!(Query2Header, search_flags), 8);
        assert_eq!(offset_of!(Query2Header, offset), 12);
        assert_eq!(offset_of!(Query2Header, max_results), 16);
        assert_eq!(offset_of!(Query2Header, request_flags), 20);
        assert_eq!(offset_of!(Query2Header, sort_type), 24);
        // EVERYTHING_IPC_LIST2: five DWORDs.
        assert_eq!(size_of::<List2Header>(), 20);
        assert_eq!(offset_of!(List2Header, total_items), 0);
        assert_eq!(offset_of!(List2Header, num_items), 4);
        assert_eq!(offset_of!(List2Header, offset), 8);
        assert_eq!(offset_of!(List2Header, request_flags), 12);
        assert_eq!(offset_of!(List2Header, sort_type), 16);
        // EVERYTHING_IPC_ITEM2: flags and the offset of its data.
        assert_eq!(size_of::<Item2>(), 8);
        assert_eq!(offset_of!(Item2, flags), 0);
        assert_eq!(offset_of!(Item2, data_offset), 4);
    }

    #[test]
    fn the_constants_are_the_ones_in_the_header() {
        assert_eq!(COPYDATA_QUERY2W, 18);
        assert_eq!(IS_DB_LOADED, 401);
        assert_eq!(REQUEST_NAME | REQUEST_PATH, 3);
        assert_eq!(REQUEST_SIZE, 0x10);
        assert_eq!(REQUEST_DATE_MODIFIED, 0x40);
        assert_eq!((ITEM_FOLDER, ITEM_DRIVE, ITEM_ROOT), (1, 2, 4));
        assert_eq!(WINDOW_CLASS, "EVERYTHING_TASKBAR_NOTIFICATION");
    }

    #[test]
    fn a_query_is_the_header_then_utf16_text_and_a_nul() {
        let header = Query2Header {
            reply_hwnd: 0x0102_0304,
            reply_copydata_message: 77,
            search_flags: 0,
            offset: 0,
            max_results: 100,
            request_flags: REQUEST_FIELDS,
            sort_type: SORT_NAME_ASCENDING,
        };
        let bytes = build_query(&header, "p\u{0457}");
        assert_eq!(bytes.len(), 28 + 3 * 2);
        assert_eq!(&bytes[0..4], &[4, 3, 2, 1]);
        assert_eq!(u32_at(&bytes, 4).unwrap(), 77);
        assert_eq!(u32_at(&bytes, 16).unwrap(), 100);
        assert_eq!(u32_at(&bytes, 20).unwrap(), REQUEST_FIELDS);
        assert_eq!(u32_at(&bytes, 24).unwrap(), 1);
        assert_eq!(&bytes[28..], &[b'p', 0, 0x57, 0x04, 0, 0]);
        // An empty search is just the NUL.
        assert_eq!(build_query(&header, "").len(), 28 + 2);
    }

    /// One field string as Everything writes it.
    fn field(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    /// An answer made with name, path, size and date modified.
    fn answer(items: &[(u32, &str, &str, i64, u64)], total: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [total, items.len() as u32, 0, REQUEST_FIELDS, 14] {
            bytes.extend_from_slice(&u32::to_le_bytes(value));
        }
        let table = bytes.len();
        bytes.resize(table + items.len() * ITEM2_SIZE, 0);
        for (index, (flags, name, path, size, time)) in items.iter().enumerate() {
            let data_at = bytes.len() as u32;
            bytes[table + index * 8..table + index * 8 + 4].copy_from_slice(&flags.to_le_bytes());
            bytes[table + index * 8 + 4..table + index * 8 + 8]
                .copy_from_slice(&data_at.to_le_bytes());
            bytes.extend(field(name));
            bytes.extend(field(path));
            bytes.extend_from_slice(&size.to_le_bytes());
            bytes.extend_from_slice(&time.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn folders_and_files_are_told_apart_and_names_keep_their_letters() {
        let bytes = answer(
            &[
                (
                    ITEM_FOLDER,
                    "pswds_OneDrive",
                    r"D:\Windows_OneDrive\OneDrive",
                    -1,
                    133_000_000_000_000_000,
                ),
                (
                    0,
                    "\u{0412}\u{0430}\u{0436}\u{043b}\u{0438}\u{0432}\u{043e}.txt",
                    r"D:\Docs",
                    2048,
                    0,
                ),
            ],
            2,
        );
        let reply = parse_list2(&bytes).unwrap();
        assert_eq!(reply.total, 2);
        assert_eq!(reply.hits.len(), 2);
        let folder = &reply.hits[0];
        assert!(folder.folder);
        assert!(!folder.drive);
        assert_eq!(folder.name, "pswds_OneDrive");
        assert_eq!(folder.size, None, "a folder without a size has none");
        assert_eq!(folder.modified, Some(133_000_000_000_000_000));
        assert_eq!(
            folder.full_path(),
            r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive"
        );
        let file = &reply.hits[1];
        assert!(!file.folder);
        assert_eq!(
            file.name,
            "\u{0412}\u{0430}\u{0436}\u{043b}\u{0438}\u{0432}\u{043e}.txt"
        );
        assert_eq!(file.size, Some(2048));
        assert_eq!(file.modified, None);
        assert_eq!(
            file.full_path(),
            "D:\\Docs\\\u{0412}\u{0430}\u{0436}\u{043b}\u{0438}\u{0432}\u{043e}.txt"
        );
    }

    #[test]
    fn a_drive_has_no_folder_above_it() {
        let bytes = answer(
            &[(ITEM_FOLDER | ITEM_DRIVE | ITEM_ROOT, "D:", "", -1, 0)],
            1,
        );
        let hit = &parse_list2(&bytes).unwrap().hits[0];
        assert!(hit.drive);
        assert_eq!(hit.full_path(), r"D:\");
    }

    #[test]
    fn an_empty_answer_has_no_hits() {
        let reply = parse_list2(&answer(&[], 0)).unwrap();
        assert_eq!(reply.total, 0);
        assert!(reply.hits.is_empty());
    }

    #[test]
    fn a_cut_answer_is_an_error_not_a_panic() {
        let full = answer(&[(0, "report.docx", r"C:\Docs", 10, 5)], 1);
        assert_eq!(parse_list2(&[]), Err(ParseError::Truncated));
        assert_eq!(parse_list2(&full[..10]), Err(ParseError::Truncated));
        // Every cut point of a good answer is refused or read, never a panic.
        for end in 0..full.len() {
            let _ = parse_list2(&full[..end]);
        }
        assert_eq!(
            parse_list2(&full[..full.len() - 1]),
            Err(ParseError::Truncated)
        );
        // An item table longer than the data.
        let mut huge = answer(&[], 0);
        huge[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_list2(&huge), Err(ParseError::Truncated));
        // A string that claims more letters than there are bytes.
        let mut lying = full.clone();
        let data_at = u32_at(&lying, LIST2_HEADER_SIZE + 4).unwrap() as usize;
        lying[data_at..data_at + 4].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        assert_eq!(parse_list2(&lying), Err(ParseError::Truncated));
    }

    #[test]
    fn fields_this_code_cannot_read_are_named() {
        let mut bytes = answer(&[], 0);
        bytes[12..16].copy_from_slice(&(REQUEST_NAME | 0x2000).to_le_bytes());
        assert_eq!(
            parse_list2(&bytes),
            Err(ParseError::UnsupportedFields(0x2000))
        );
    }

    #[test]
    fn a_subset_of_fields_is_read_in_order() {
        // Name only.
        let mut bytes = Vec::new();
        for value in [1u32, 1, 0, REQUEST_NAME, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&28u32.to_le_bytes());
        bytes.extend(field("only.txt"));
        let hit = &parse_list2(&bytes).unwrap().hits[0];
        assert_eq!(hit.name, "only.txt");
        assert_eq!(hit.path, "");
        assert_eq!(hit.full_path(), r"only.txt\");
    }

    #[test]
    fn a_parse_error_says_what_is_wrong_in_plain_words() {
        assert_eq!(ParseError::Truncated.to_string(), "the answer is cut short");
        assert_eq!(
            ParseError::UnsupportedFields(0x10_0000).to_string(),
            "the answer has fields this code cannot read (0x100000)"
        );
    }
}
