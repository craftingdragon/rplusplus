// SPDX-License-Identifier: GPL-3.0-or-later
#include "notes.h"
#include <QApplication>
#include <QDir>
#include <QFile>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QVBoxLayout>
#include <QLabel>
#include <stdexcept>
#include <memory>
static NotesView view(const QByteArray &s) { return {reinterpret_cast<const unsigned char*>(s.constData()), size_t(s.size())}; }
struct Error { NotesBuffer b{}; ~Error(){ notes_free(b); } };
class Window: public QWidget {
    Q_OBJECT
    std::unique_ptr<NotesHandle, decltype(&notes_close)> db{nullptr,notes_close};
    QListWidget *list=new QListWidget(this);
    QPlainTextEdit *editor=new QPlainTextEdit(this);
    QLabel *status=new QLabel(this);
    bool check(int result, const Error &e) {
        if(result) status->setText(QString::fromUtf8(reinterpret_cast<const char*>(e.b.data),qsizetype(e.b.len)));
        return result==0;
    }
    static int32_t row(void *user,int64_t id,NotesView text) noexcept {
        try { auto self=static_cast<Window*>(user);
            auto item=new QListWidgetItem(QString::fromUtf8(reinterpret_cast<const char*>(text.data),qsizetype(text.len)),self->list);
            item->setData(Qt::UserRole,qlonglong(id)); return 0;
        } catch (...) { return 1; }
    }
public:
    explicit Window(const QString &path) {
        auto layout=new QVBoxLayout(this); layout->addWidget(list); layout->addWidget(editor);
        auto fresh=new QPushButton("New",this), save=new QPushButton("Save",this), remove=new QPushButton("Delete",this);
        layout->addWidget(fresh); layout->addWidget(save); layout->addWidget(remove); layout->addWidget(status);
        connect(fresh,&QPushButton::clicked,this,[this]{list->setCurrentRow(-1);editor->clear();});
        connect(save,&QPushButton::clicked,this,&Window::saveNote);
        connect(remove,&QPushButton::clicked,this,&Window::deleteNote);
        connect(list,&QListWidget::currentTextChanged,editor,&QPlainTextEdit::setPlainText);
        Error e; NotesHandle *raw=nullptr;
        if(check(notes_open(view(path.toUtf8()),&raw,&e.b),e)) { db.reset(raw); refresh(); }
        save->setEnabled(bool(db)); remove->setEnabled(bool(db));
        resize(600,500); setWindowTitle("R++ Notes — Qt / Rust / SQLite");
    }
    void refresh(){ list->clear(); Error e; check(notes_list(db.get(),row,this,&e.b),e); }
    void saveNote(){ Error e; auto item=list->currentItem(); auto text=editor->toPlainText().toUtf8();
        if(check(notes_save(db.get(),item?item->data(Qt::UserRole).toLongLong():0,view(text),&e.b),e))refresh(); }
    void deleteNote(){ if(auto item=list->currentItem()){ Error e; if(check(notes_delete(db.get(),item->data(Qt::UserRole).toLongLong(),&e.b),e))refresh(); } }
    void exercise(){
        if(!db)throw std::runtime_error("database failed to open");
        editor->setPlainText(QString::fromUtf8("Hello 🦀")); saveNote();
        if(list->count()!=1 || list->item(0)->text()!=QString::fromUtf8("Hello 🦀"))throw std::runtime_error("save failed");
        list->setCurrentRow(0); editor->setPlainText(QString::fromUtf8("updated 🦀")); saveNote();
        if(list->count()!=1 || list->item(0)->text()!=QString::fromUtf8("updated 🦀"))throw std::runtime_error("update failed");
    }
    QString firstText()const{return list->count()?list->item(0)->text():QString();}
    int count()const{return list->count();}
    void removeFirst(){list->setCurrentRow(0);deleteNote();}
};
int main(int argc,char **argv){
    QApplication app(argc,argv);
    if(app.arguments().contains("--self-test")){
        QTemporaryDir dir; if(!dir.isValid())return 1; auto path=dir.filePath("notes.sqlite");
        try { {Window w(path);w.exercise();app.processEvents();}
            {Window w(path);if(w.count()!=1 || w.firstText()!=QString::fromUtf8("updated 🦀"))return 2;w.removeFirst();if(w.count()!=0)return 3;}
            Error e; NotesHandle *h=nullptr; auto invalid=dir.filePath("missing/notes.sqlite").toUtf8();
            if(notes_open(view(invalid),&h,&e.b)==0 || h || !e.b.len)return 4;
        }catch(const std::exception &){return 5;} return 0;
    }
    auto storage=QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);QDir().mkpath(storage);
    Window window(storage+"/notes.sqlite");window.show();return app.exec();
}
#include "main.moc"
