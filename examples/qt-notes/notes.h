// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct NotesHandle NotesHandle;
typedef struct { const unsigned char *data; size_t len; } NotesView;
typedef struct { unsigned char *data; size_t len; } NotesBuffer;
/* Views are borrowed for the call. Handles are single-threaded; callbacks must
   not re-enter the backend. Every nonempty error buffer must be freed once. */
typedef int32_t (*NotesRow)(void *, int64_t, NotesView);
int32_t notes_open(NotesView, NotesHandle **, NotesBuffer *);
void notes_close(NotesHandle *);
void notes_free(NotesBuffer);
int32_t notes_save(NotesHandle *, int64_t, NotesView, NotesBuffer *);
int32_t notes_delete(NotesHandle *, int64_t, NotesBuffer *);
int32_t notes_list(NotesHandle *, NotesRow, void *, NotesBuffer *);
#ifdef __cplusplus
}
#endif
