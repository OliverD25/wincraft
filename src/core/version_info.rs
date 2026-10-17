//! Strings from a program's version resource.

use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

use crate::core::wide;

/// One string of the version resource, such as "FileDescription" or
/// "ProductName"; None when the file has none or it is empty.
pub fn string(path: &str, key: &str) -> Option<String> {
    let name = wide(path);
    let mut handle = 0u32;
    let size = unsafe { GetFileVersionInfoSizeW(name.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(name.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let query = |sub: &str| -> Option<(*const u8, usize)> {
        let sub = wide(sub);
        let mut pointer: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let ok =
            unsafe { VerQueryValueW(block.as_ptr().cast(), sub.as_ptr(), &mut pointer, &mut len) };
        (ok != 0 && !pointer.is_null() && len > 0).then_some((pointer as *const u8, len as usize))
    };
    // The first language the resource lists, then US English as most
    // programs ship it.
    let mut languages = Vec::new();
    if let Some((pointer, len)) = query(r"\VarFileInfo\Translation") {
        if len >= 4 {
            let words = unsafe { std::slice::from_raw_parts(pointer as *const u16, 2) };
            languages.push(format!("{:04x}{:04x}", words[0], words[1]));
        }
    }
    languages.push("040904b0".to_string());
    languages.into_iter().find_map(|language| {
        let (pointer, len) = query(&format!(r"\StringFileInfo\{language}\{key}"))?;
        let units = unsafe { std::slice::from_raw_parts(pointer as *const u16, len) };
        let text = String::from_utf16_lossy(units);
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| text.to_string())
    })
}
