// SPDX-License-Identifier: GPL-3.0-or-later
use notes_core::Database;

#if RPP_NOTES_COUNT
pub(super) fn read(db: &Database) -> Result<Option<i64>, String> {
    db.count().map(Some)
}
#else
pub(super) fn read(_db: &Database) -> Result<Option<i64>, String> {
    Ok(None)
}
#endif
