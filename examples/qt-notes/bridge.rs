// SPDX-License-Identifier: GPL-3.0-or-later
use notes_core::Database;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};
#[cfg(rpp_notes_preprocess)]
#[path = "count_preprocessed.rs"]
mod count;
#[cfg(not(rpp_notes_preprocess))]
mod count {
    use notes_core::Database;

    pub(super) fn read(_db: &Database) -> Result<Option<i64>, String> {
        Ok(None)
    }
}
#[repr(C)]
pub struct View {
    data: *const u8,
    len: usize,
}
#[repr(C)]
pub struct Buffer {
    data: *mut u8,
    len: usize,
}
unsafe fn text<'a>(v: View) -> Result<&'a str, String> {
    if v.len == 0 {
        return Ok("");
    }
    if v.data.is_null() || v.len > isize::MAX as usize {
        return Err("invalid byte view".into());
    }
    unsafe { std::str::from_utf8(std::slice::from_raw_parts(v.data, v.len)) }
        .map_err(|e| e.to_string())
}
unsafe fn boundary(err: *mut Buffer, f: impl FnOnce() -> Result<(), String>) -> i32 {
    if err.is_null() {
        return 1;
    }
    unsafe {
        *err = Buffer {
            data: ptr::null_mut(),
            len: 0,
        };
    }
    match catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err("Rust backend panic".into())) {
        Ok(()) => 0,
        Err(e) => {
            let bytes = e.into_bytes().into_boxed_slice();
            let len = bytes.len();
            unsafe {
                *err = Buffer {
                    data: Box::into_raw(bytes) as *mut u8,
                    len,
                };
            }
            1
        }
    }
}
unsafe fn db<'a>(h: *mut Database) -> Result<&'a Database, String> {
    unsafe { h.as_ref() }.ok_or("null database handle".into())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_open(path: View, out: *mut *mut Database, err: *mut Buffer) -> i32 {
    unsafe {
        boundary(err, || {
            if out.is_null() {
                return Err("null output handle".into());
            }
            *out = ptr::null_mut();
            *out = Box::into_raw(Box::new(Database::open(text(path)?)?));
            Ok(())
        })
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_close(h: *mut Database) {
    if !h.is_null() {
        unsafe {
            drop(Box::from_raw(h));
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_free(b: Buffer) {
    if !b.data.is_null() {
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(b.data, b.len)));
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_save(h: *mut Database, id: i64, v: View, err: *mut Buffer) -> i32 {
    unsafe { boundary(err, || db(h)?.save(id, text(v)?)) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_delete(h: *mut Database, id: i64, err: *mut Buffer) -> i32 {
    unsafe { boundary(err, || db(h)?.delete(id)) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_count(h: *mut Database, out: *mut i64, err: *mut Buffer) -> i32 {
    unsafe {
        boundary(err, || {
            let out = out.as_mut().ok_or("null output count")?;
            *out = count::read(db(h)?)?.unwrap_or(-1);
            Ok(())
        })
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notes_list(
    h: *mut Database,
    cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, View) -> i32>,
    user: *mut std::ffi::c_void,
    err: *mut Buffer,
) -> i32 {
    unsafe {
        boundary(err, || {
            let cb = cb.ok_or("null callback")?;
            db(h)?.list(|id, s| {
                if cb(
                    user,
                    id,
                    View {
                        data: s.as_ptr(),
                        len: s.len(),
                    },
                ) == 0
                {
                    Ok(())
                } else {
                    Err("callback cancelled listing".into())
                }
            })
        })
    }
}
