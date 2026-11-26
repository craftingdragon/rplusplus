// SPDX-License-Identifier: GPL-3.0-or-later
#include <notes.h>
#include <cstdlib>
#include <cstring>
#include <memory>

struct Error {
    NotesBuffer buffer{};
    ~Error(){notes_free(buffer);}
};

static int32_t row(void *user,int64_t,NotesView text){
    ++*static_cast<int*>(user);
    return text.len==5 && std::memcmp(text.data,"hello",5)==0 ? 0 : 1;
}

static int32_t cancel(void *,int64_t,NotesView){return 1;}

int main(){
    for(int i=0;i<100;++i){
        Error error;
        if(notes_save(nullptr,0,{nullptr,0},&error.buffer)==0 || !error.buffer.data)return 2;
    }
    NotesHandle *raw=nullptr;
    const unsigned char memory[]=":memory:", text[]="hello";
    {
        Error error;
        if(notes_open({memory,sizeof(memory)-1},&raw,&error.buffer))return 1;
    }
    std::unique_ptr<NotesHandle,decltype(&notes_close)> db(raw,notes_close);
    bool enabled;
    {
        Error error;
        int64_t count=-2;
        if(notes_count(db.get(),&count,&error.buffer) || error.buffer.len)return 6;
        const char *expected=std::getenv("RPP_NOTES_EXPECT_COUNT");
        enabled=expected ? std::strcmp(expected,"1")==0 : count==0;
        if(count!=(enabled ? 0 : NOTES_COUNT_UNAVAILABLE))return 7;
    }
    {
        Error error;
        if(notes_save(db.get(),0,{text,sizeof(text)-1},&error.buffer))return 3;
    }
    {
        Error error;
        int rows=0;
        if(notes_list(db.get(),row,&rows,&error.buffer) || rows!=1)return 3;
    }
    {
        Error error;
        int64_t count=-2;
        if(notes_count(db.get(),&count,&error.buffer)
           || count!=(enabled ? 1 : NOTES_COUNT_UNAVAILABLE))return 8;
    }
    {
        Error error;
        int64_t count=42;
        if(notes_count(nullptr,&count,&error.buffer)==0 || !error.buffer.len || count!=42)return 9;
    }
    {
        Error error;
        if(notes_count(db.get(),nullptr,&error.buffer)==0 || !error.buffer.len)return 10;
    }
    {
        Error error;
        const unsigned char invalid[]={0xff};
        if(notes_save(db.get(),0,{invalid,1},&error.buffer)==0 || !error.buffer.len)return 4;
    }
    {
        Error error;
        if(notes_list(db.get(),nullptr,nullptr,&error.buffer)==0 || !error.buffer.len)return 5;
    }
    {
        Error error;
        if(notes_list(db.get(),cancel,nullptr,&error.buffer)==0 || !error.buffer.len)return 11;
    }
    return 0;
}
