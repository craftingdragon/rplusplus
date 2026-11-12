// SPDX-License-Identifier: GPL-3.0-or-later
#include <notes.h>
#include <cstring>
static int32_t row(void *user,int64_t,NotesView text){++*static_cast<int*>(user);return text.len==5?0:1;}
int main(){
    for(int i=0;i<100;++i){
        NotesBuffer error{};
        if(notes_save(nullptr,0,{nullptr,0},&error)==0 || !error.data)return 2;
        notes_free(error);
    }
    NotesBuffer error{};NotesHandle *db=nullptr;
    const unsigned char memory[]=":memory:", text[]="hello";
    if(notes_open({memory,sizeof(memory)-1},&db,&error))return 1;
    int status=notes_save(db,0,{text,sizeof(text)-1},&error);int count=0;
    if(!status)status=notes_list(db,row,&count,&error);
    notes_free(error);
    if(status || count!=1){notes_close(db);return 3;}
    const unsigned char invalid[]={0xff};
    if(notes_save(db,0,{invalid,1},&error)==0)return 4;
    notes_free(error);
    if(notes_list(db,nullptr,&count,&error)==0)return 5;
    notes_free(error);
    notes_close(db);return 0;
}
