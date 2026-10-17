//! Asks the Windows Search index a question in its SQL, through the OLE DB
//! provider `Search.CollatorDSO` that Windows ships. windows-sys has no COM
//! wrappers, so the six interfaces a query needs are declared here as
//! vtables, on the shared [`ComPtr`].
//!
//! The steps are the documented ones: make the data source, `Initialize` it,
//! open a session, make a command, give it the SQL, `Execute` it for an
//! `IRowset`, and read each row through an accessor that binds three columns
//! to a small struct. COM must be running on the calling thread.

use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::time::Instant;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
use windows_sys::Win32::System::Com::{CLSIDFromProgID, CoTaskMemFree, CLSCTX_ALL};
use windows_sys::Win32::System::Search::{
    DBBINDING, DBMEMOWNER_CLIENTOWNED, DBMEMOWNER_PROVIDEROWNED, DBPART_STATUS, DBPART_VALUE,
    DBTYPE_BYREF, DBTYPE_FILETIME, DBTYPE_WSTR,
};

use crate::core::com::{self, ComPtr};
use crate::core::{from_wide_ptr, wide};

const PROG_ID: &str = "Search.CollatorDSO";

const IID_IDB_INITIALIZE: GUID = GUID::from_u128(0x0c733a8b_2a1c_11ce_ade5_00aa0044773d);
const IID_IDB_CREATE_SESSION: GUID = GUID::from_u128(0x0c733a5d_2a1c_11ce_ade5_00aa0044773d);
const IID_IDB_CREATE_COMMAND: GUID = GUID::from_u128(0x0c733a1d_2a1c_11ce_ade5_00aa0044773d);
const IID_ICOMMAND_TEXT: GUID = GUID::from_u128(0x0c733a27_2a1c_11ce_ade5_00aa0044773d);
const IID_IROWSET: GUID = GUID::from_u128(0x0c733a7c_2a1c_11ce_ade5_00aa0044773d);
const IID_IACCESSOR: GUID = GUID::from_u128(0x0c733a8c_2a1c_11ce_ade5_00aa0044773d);
/// The dialect of a plain SQL string.
const DBGUID_DEFAULT: GUID = GUID::from_u128(0xc8b521fb_5cf3_11ce_ade5_00aa0044773d);

/// `DBACCESSOR_ROWDATA`.
const ACCESSOR_ROWDATA: u32 = 2;
/// `DBSTATUS_S_OK`: the column has a value.
const STATUS_OK: u32 = 0;
/// Rows asked for at a time.
const BATCH: isize = 64;

#[repr(C)]
struct IDbInitializeVtbl {
    _base: IUnknown_Vtbl,
    initialize: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    uninitialize: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

/// `IDBCreateSession` and `IDBCreateCommand` have the same one method.
#[repr(C)]
struct ICreateVtbl {
    _base: IUnknown_Vtbl,
    create: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const GUID,
        *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(C)]
struct ICommandTextVtbl {
    _base: IUnknown_Vtbl,
    _cancel: unsafe extern "system" fn(),
    execute: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const GUID,
        *mut c_void,
        *mut isize,
        *mut *mut c_void,
    ) -> HRESULT,
    _get_db_session: unsafe extern "system" fn(),
    _get_command_text: unsafe extern "system" fn(),
    set_command_text: unsafe extern "system" fn(*mut c_void, *const GUID, PCWSTR) -> HRESULT,
}

#[repr(C)]
struct IAccessorVtbl {
    _base: IUnknown_Vtbl,
    _add_ref_accessor: unsafe extern "system" fn(),
    create_accessor: unsafe extern "system" fn(
        *mut c_void,
        u32,
        usize,
        *const DBBINDING,
        usize,
        *mut usize,
        *mut u32,
    ) -> HRESULT,
    _get_bindings: unsafe extern "system" fn(),
    release_accessor: unsafe extern "system" fn(*mut c_void, usize, *mut u32) -> HRESULT,
}

#[repr(C)]
struct IRowsetVtbl {
    _base: IUnknown_Vtbl,
    _add_ref_rows: unsafe extern "system" fn(),
    get_data: unsafe extern "system" fn(*mut c_void, usize, usize, *mut c_void) -> HRESULT,
    get_next_rows: unsafe extern "system" fn(
        *mut c_void,
        usize,
        isize,
        isize,
        *mut usize,
        *mut *mut usize,
    ) -> HRESULT,
    release_rows: unsafe extern "system" fn(
        *mut c_void,
        usize,
        *const usize,
        *const u32,
        *mut u32,
        *mut u32,
    ) -> HRESULT,
    _restart_position: unsafe extern "system" fn(),
}

/// One row as the accessor writes it. The strings are owned by the provider
/// and stay valid until the row is released.
#[repr(C)]
struct RowData {
    url: PCWSTR,
    url_status: u32,
    kind: PCWSTR,
    kind_status: u32,
    modified: u64,
    modified_status: u32,
}

/// The three columns of every query, in the order `RowData` has them.
pub const COLUMNS: &str = "System.ItemUrl, System.ItemType, System.DateModified";

/// What a row of the index says, before it is turned into a hit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawRow {
    pub url: Option<String>,
    pub kind: Option<String>,
    /// Windows FILETIME.
    pub modified: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The index cannot be reached: the Windows Search service is off, or
    /// the provider is not installed.
    Unavailable(HRESULT),
    /// The index took the question and could not answer it.
    Query(HRESULT),
    TimedOut,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unavailable(hr) => write!(f, "cannot reach the index ({})", com::hex(*hr)),
            Failure::Query(hr) => write!(f, "the index refused the search ({})", com::hex(*hr)),
            Failure::TimedOut => f.write_str("the index took too long"),
        }
    }
}

/// An open session on the index.
pub struct Connection {
    /// Dropped before the data source is uninitialised.
    session: Option<ComPtr>,
    source: ComPtr,
}

impl Connection {
    pub fn open() -> Result<Self, Failure> {
        let mut clsid = com::guid(0, 0, 0, [0; 8]);
        let hr = unsafe { CLSIDFromProgID(wide(PROG_ID).as_ptr(), &mut clsid) };
        com::check(hr).map_err(Failure::Unavailable)?;
        let source = ComPtr::create(&clsid, &IID_IDB_INITIALIZE, CLSCTX_ALL)
            .map_err(Failure::Unavailable)?;
        let hr = unsafe { (source.vtable::<IDbInitializeVtbl>().initialize)(source.as_raw()) };
        com::check(hr).map_err(Failure::Unavailable)?;
        // From here the source must be uninitialised, which Drop does.
        let mut connection = Connection {
            session: None,
            source,
        };
        let creator = connection
            .source
            .query(&IID_IDB_CREATE_SESSION)
            .ok_or(Failure::Unavailable(0))?;
        connection.session =
            Some(make(&creator, &IID_IDB_CREATE_COMMAND).map_err(Failure::Unavailable)?);
        Ok(connection)
    }

    /// Runs `sql` (which must select [`COLUMNS`]) and reads at most
    /// `most` rows. Gives up with `TimedOut` once `deadline` has passed
    /// between two batches.
    pub fn query(&self, sql: &str, most: usize, deadline: Instant) -> Result<Vec<RawRow>, Failure> {
        let session = self.session.as_ref().ok_or(Failure::Unavailable(0))?;
        let command = make(session, &IID_ICOMMAND_TEXT).map_err(Failure::Query)?;
        let hr = unsafe {
            (command.vtable::<ICommandTextVtbl>().set_command_text)(
                command.as_raw(),
                &DBGUID_DEFAULT,
                wide(sql).as_ptr(),
            )
        };
        com::check(hr).map_err(Failure::Query)?;
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (command.vtable::<ICommandTextVtbl>().execute)(
                command.as_raw(),
                std::ptr::null_mut(),
                &IID_IROWSET,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut raw,
            )
        };
        com::check(hr).map_err(Failure::Query)?;
        let rowset = ComPtr::from_raw(raw).ok_or(Failure::Query(hr))?;
        let accessor = Accessor::create(&rowset)?;
        read_rows(&rowset, &accessor, most, deadline)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.session = None;
        unsafe { (self.source.vtable::<IDbInitializeVtbl>().uninitialize)(self.source.as_raw()) };
    }
}

/// `IDBCreateSession::CreateSession` or `IDBCreateCommand::CreateCommand`:
/// the object they make, as the interface `iid`.
fn make(creator: &ComPtr, iid: &GUID) -> Result<ComPtr, HRESULT> {
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (creator.vtable::<ICreateVtbl>().create)(
            creator.as_raw(),
            std::ptr::null_mut(),
            iid,
            &mut raw,
        )
    };
    com::check(hr)?;
    ComPtr::from_raw(raw).ok_or(hr)
}

fn binding(ordinal: usize, value_at: usize, status_at: usize, ty: u16, owner: i32) -> DBBINDING {
    let mut binding: DBBINDING = unsafe { std::mem::zeroed() };
    binding.iOrdinal = ordinal;
    binding.obValue = value_at;
    binding.obStatus = status_at;
    binding.dwPart = (DBPART_VALUE | DBPART_STATUS) as u32;
    binding.dwMemOwner = owner as u32;
    binding.wType = ty;
    binding
}

/// The three bindings: two strings the provider owns, and a FILETIME.
fn bindings() -> [DBBINDING; 3] {
    let string = DBTYPE_WSTR as u16 | DBTYPE_BYREF as u16;
    [
        binding(
            1,
            offset_of!(RowData, url),
            offset_of!(RowData, url_status),
            string,
            DBMEMOWNER_PROVIDEROWNED,
        ),
        binding(
            2,
            offset_of!(RowData, kind),
            offset_of!(RowData, kind_status),
            string,
            DBMEMOWNER_PROVIDEROWNED,
        ),
        binding(
            3,
            offset_of!(RowData, modified),
            offset_of!(RowData, modified_status),
            DBTYPE_FILETIME as u16,
            DBMEMOWNER_CLIENTOWNED,
        ),
    ]
}

struct Accessor {
    owner: ComPtr,
    handle: usize,
}

impl Accessor {
    fn create(rowset: &ComPtr) -> Result<Self, Failure> {
        let owner = rowset.query(&IID_IACCESSOR).ok_or(Failure::Query(0))?;
        let list = bindings();
        let mut handle = 0usize;
        let hr = unsafe {
            (owner.vtable::<IAccessorVtbl>().create_accessor)(
                owner.as_raw(),
                ACCESSOR_ROWDATA,
                list.len(),
                list.as_ptr(),
                size_of::<RowData>(),
                &mut handle,
                std::ptr::null_mut(),
            )
        };
        com::check(hr).map_err(Failure::Query)?;
        Ok(Accessor { owner, handle })
    }
}

impl Drop for Accessor {
    fn drop(&mut self) {
        unsafe {
            (self.owner.vtable::<IAccessorVtbl>().release_accessor)(
                self.owner.as_raw(),
                self.handle,
                std::ptr::null_mut(),
            )
        };
    }
}

fn read_rows(
    rowset: &ComPtr,
    accessor: &Accessor,
    most: usize,
    deadline: Instant,
) -> Result<Vec<RawRow>, Failure> {
    let mut found = Vec::new();
    while found.len() < most {
        if Instant::now() >= deadline {
            return Err(Failure::TimedOut);
        }
        let mut obtained = 0usize;
        let mut rows: *mut usize = std::ptr::null_mut();
        let hr = unsafe {
            (rowset.vtable::<IRowsetVtbl>().get_next_rows)(
                rowset.as_raw(),
                0,
                0,
                BATCH,
                &mut obtained,
                &mut rows,
            )
        };
        com::check(hr).map_err(Failure::Query)?;
        if obtained == 0 || rows.is_null() {
            break;
        }
        let handles = unsafe { std::slice::from_raw_parts(rows, obtained) };
        for handle in handles {
            let mut data: RowData = unsafe { std::mem::zeroed() };
            let hr = unsafe {
                (rowset.vtable::<IRowsetVtbl>().get_data)(
                    rowset.as_raw(),
                    *handle,
                    accessor.handle,
                    &mut data as *mut RowData as *mut c_void,
                )
            };
            if com::ok(hr) {
                found.push(raw_row(&data));
            }
        }
        unsafe {
            (rowset.vtable::<IRowsetVtbl>().release_rows)(
                rowset.as_raw(),
                obtained,
                rows,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            CoTaskMemFree(rows as *const c_void);
        }
    }
    found.truncate(most);
    Ok(found)
}

/// Reads a row while its strings are still valid.
fn raw_row(data: &RowData) -> RawRow {
    let text = |at: PCWSTR, status: u32| {
        (status == STATUS_OK && !at.is_null()).then(|| unsafe { from_wide_ptr(at) })
    };
    RawRow {
        url: text(data.url, data.url_status),
        kind: text(data.kind, data.kind_status),
        modified: (data.modified_status == STATUS_OK && data.modified != 0)
            .then_some(data.modified),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    #[test]
    fn the_row_struct_has_the_offsets_the_bindings_use() {
        let list = bindings();
        assert_eq!(list[0].obValue, 0);
        assert_eq!(list[0].obStatus, 8);
        assert_eq!(list[1].obValue, 16);
        assert_eq!(list[2].obValue, 32);
        assert_eq!(list[2].obStatus, 40);
        assert_eq!(size_of::<RowData>(), 48);
        assert_eq!(
            list.iter().map(|b| b.iOrdinal).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            list[0].wType, 0x4082,
            "a string the provider owns, by pointer"
        );
        assert_eq!(list[2].wType, 64, "a FILETIME");
        assert_eq!(list[0].dwPart, 5, "value and status");
    }

    #[test]
    fn a_row_keeps_only_the_columns_that_have_a_value() {
        let url = wide("file:C:/a.txt");
        let data = RowData {
            url: url.as_ptr(),
            url_status: STATUS_OK,
            kind: url.as_ptr(),
            kind_status: 3,
            modified: 5,
            modified_status: STATUS_OK,
        };
        assert_eq!(
            raw_row(&data),
            RawRow {
                url: Some("file:C:/a.txt".to_string()),
                kind: None,
                modified: Some(5),
            }
        );
        let empty = RowData {
            url: std::ptr::null(),
            url_status: STATUS_OK,
            kind: std::ptr::null(),
            kind_status: STATUS_OK,
            modified: 0,
            modified_status: STATUS_OK,
        };
        assert_eq!(raw_row(&empty), RawRow::default());
    }

    /// Reads only: it asks the index of this PC for one row, if there is an
    /// index. Without Windows Search the answer is `Unavailable`, which is
    /// the other thing this checks.
    #[test]
    fn the_index_answers_or_is_reported_unavailable() {
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let sql = format!(
            "SELECT TOP 1 {COLUMNS} FROM SystemIndex WHERE SCOPE='file:' ORDER BY System.DateModified DESC"
        );
        match Connection::open() {
            Ok(connection) => {
                let deadline = Instant::now() + Duration::from_secs(5);
                match connection.query(&sql, 1, deadline) {
                    Ok(rows) => assert!(rows.len() <= 1),
                    Err(failure) => println!("the index refused the test search: {failure}"),
                }
                let expired = Instant::now() - Duration::from_millis(1);
                assert_eq!(connection.query(&sql, 1, expired), Err(Failure::TimedOut));
            }
            Err(Failure::Unavailable(_)) => {}
            Err(other) => panic!("opening the index failed oddly: {other}"),
        }
        unsafe { CoUninitialize() };
    }
}
