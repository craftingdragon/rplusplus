// SPDX-License-Identifier: GPL-3.0-or-later
#include <notes.h>
#include <cstring>
static int32_t row(void *user,int64_t,NotesView text){++*static_cast<int*>(user);return text.len==5?0:1;}
int main(){
    NotesBuffer error{};NotesHandle *db=nullptr;
    const unsigned char memory[]=":memory:", text[]="hello";
    if(notes_open({memory,sizeof(memory)-1},&db,&error))return 1;
    int status=notes_save(db,0,{text,sizeof(text)-1},&error);int count=0;
    if(!status)status=notes_list(db,row,&count,&error);
    notes_free(error);notes_close(db);return status || count!=1;
}
