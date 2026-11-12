// SPDX-License-Identifier: GPL-3.0-or-later
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
};
unsafe extern "C" {
    fn sqlite3_open(path: *const c_char, db: *mut *mut c_void) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
    fn sqlite3_exec(
        db: *mut c_void,
        sql: *const c_char,
        cb: *const c_void,
        user: *mut c_void,
        err: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        n: c_int,
        stmt: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut c_void,
        index: c_int,
        text: *const c_char,
        len: c_int,
        destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut c_void, index: c_int, value: i64) -> c_int;
    fn sqlite3_step(stmt: *mut c_void) -> c_int;
    fn sqlite3_column_int64(stmt: *mut c_void, index: c_int) -> i64;
    fn sqlite3_column_text(stmt: *mut c_void, index: c_int) -> *const c_char;
    fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
}
pub struct Database(*mut c_void);
impl Drop for Database {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close(self.0);
        }
    }
}
struct Statement(*mut c_void);
impl Drop for Statement {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.0);
        }
    }
}
impl Database {
    fn error(&self) -> String {
        unsafe {
            CStr::from_ptr(sqlite3_errmsg(self.0))
                .to_string_lossy()
                .into_owned()
        }
    }
    pub fn open(path: &str) -> Result<Self, String> {
        let path = CString::new(path).map_err(|_| "NUL in database path")?;
        let mut raw = ptr::null_mut();
        let status = unsafe { sqlite3_open(path.as_ptr(), &mut raw) };
        if raw.is_null() {
            return Err("SQLite allocation failed".into());
        }
        let db = Self(raw);
        if status != 0 {
            return Err(db.error());
        }
        let schema =
            c"CREATE TABLE IF NOT EXISTS notes(id INTEGER PRIMARY KEY, text TEXT NOT NULL)";
        if unsafe {
            sqlite3_exec(
                raw,
                schema.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } != 0
        {
            return Err(db.error());
        }
        Ok(db)
    }
    fn prepare(&self, sql: &CStr) -> Result<Statement, String> {
        let mut raw = ptr::null_mut();
        if unsafe { sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut raw, ptr::null_mut()) } != 0 {
            return Err(self.error());
        }
        Ok(Statement(raw))
    }
    pub fn save(&self, id: i64, text: &str) -> Result<(), String> {
        let text = CString::new(text).map_err(|_| "NUL in note")?;
        let stmt=self.prepare(c"INSERT INTO notes(id,text) VALUES(NULLIF(?1,0),?2) ON CONFLICT(id) DO UPDATE SET text=excluded.text")?;
        // SQLITE_STATIC: text remains alive until after step and finalization.
        let status = unsafe {
            let bound = sqlite3_bind_int64(stmt.0, 1, id);
            if bound != 0 {
                return Err(self.error());
            }
            sqlite3_bind_text(stmt.0, 2, text.as_ptr(), -1, None)
        };
        let result = if status == 0 && unsafe { sqlite3_step(stmt.0) } == 101 {
            Ok(())
        } else {
            Err(self.error())
        };
        drop(stmt);
        result
    }
    pub fn delete(&self, id: i64) -> Result<(), String> {
        let stmt = self.prepare(c"DELETE FROM notes WHERE id=?1")?;
        if unsafe { sqlite3_bind_int64(stmt.0, 1, id) } != 0
            || unsafe { sqlite3_step(stmt.0) } != 101
        {
            return Err(self.error());
        }
        Ok(())
    }
    pub fn list(&self, mut row: impl FnMut(i64, &str) -> Result<(), String>) -> Result<(), String> {
        let stmt = self.prepare(c"SELECT id,text FROM notes ORDER BY id")?;
        loop {
            match unsafe { sqlite3_step(stmt.0) } {
                100 => {
                    let raw = unsafe { sqlite3_column_text(stmt.0, 1) };
                    if raw.is_null() {
                        return Err(self.error());
                    }
                    let text = unsafe { CStr::from_ptr(raw) }
                        .to_str()
                        .map_err(|e| e.to_string())?;
                    row(unsafe { sqlite3_column_int64(stmt.0, 0) }, text)?;
                }
                101 => return Ok(()),
                _ => return Err(self.error()),
            }
        }
    }
}
