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
    QLabel *total=new QLabel(this);
    QPushButton *save=new QPushButton("Save",this);
    QPushButton *remove=new QPushButton("Delete",this);
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
        auto fresh=new QPushButton("New",this);
        layout->addWidget(fresh); layout->addWidget(save); layout->addWidget(remove);
        layout->addWidget(total); layout->addWidget(status);
        total->hide();
        connect(fresh,&QPushButton::clicked,this,[this]{list->setCurrentRow(-1);editor->clear();});
        connect(save,&QPushButton::clicked,this,&Window::saveNote);
        connect(remove,&QPushButton::clicked,this,&Window::deleteNote);
        connect(list,&QListWidget::currentTextChanged,editor,&QPlainTextEdit::setPlainText);
        Error e; NotesHandle *raw=nullptr;
        if(check(notes_open(view(path.toUtf8()),&raw,&e.b),e)) { db.reset(raw); refresh(); }
        save->setEnabled(bool(db)); remove->setEnabled(bool(db));
        resize(600,500); setWindowTitle("R++ Notes — Qt / Rust / SQLite");
    }
    void refresh(){
        total->hide();
        list->clear();
        Error listing;
        if(!check(notes_list(db.get(),row,this,&listing.b),listing))return;
        Error counting;
        int64_t n=NOTES_COUNT_UNAVAILABLE;
        if(!check(notes_count(db.get(),&n,&counting.b),counting) || n==NOTES_COUNT_UNAVAILABLE)return;
        total->setText(n==1 ? QStringLiteral("1 note") : QStringLiteral("%1 notes").arg(n));
        total->show();
    }
    void saveNote(){ Error e; auto item=list->currentItem(); auto text=editor->toPlainText().toUtf8();
        if(check(notes_save(db.get(),item?item->data(Qt::UserRole).toLongLong():0,view(text),&e.b),e))refresh(); }
    void deleteNote(){ if(auto item=list->currentItem()){ Error e; if(check(notes_delete(db.get(),item->data(Qt::UserRole).toLongLong(),&e.b),e))refresh(); } }
    void expectCount(bool enabled,int expected)const{
        const auto label=expected==1 ? QStringLiteral("1 note") : QStringLiteral("%1 notes").arg(expected);
        if(list->count()!=expected || total->isHidden()==enabled || (enabled && total->text()!=label))
            throw std::runtime_error("note count or label visibility mismatch");
    }
    bool countVisible()const{return !total->isHidden();}
    bool showsOpenError()const{return !db && !save->isEnabled() && !remove->isEnabled()
        && !status->text().isEmpty() && total->isHidden();}
    void exercise(bool enabled){
        if(!db)throw std::runtime_error("database failed to open");
        expectCount(enabled,0);
        editor->setPlainText(QString::fromUtf8("Grüezi — 日本語 🦀")); saveNote();
        expectCount(enabled,1);
        if(firstText()!=QString::fromUtf8("Grüezi — 日本語 🦀"))throw std::runtime_error("save failed");
        list->setCurrentRow(-1);
        editor->setPlainText(QStringLiteral("Second note")); saveNote();
        expectCount(enabled,2);
        list->setCurrentRow(0); editor->setPlainText(QString::fromUtf8("Überarbeitet — 中文 🦀")); saveNote();
        expectCount(enabled,2);
        if(firstText()!=QString::fromUtf8("Überarbeitet — 中文 🦀"))throw std::runtime_error("update failed");
    }
    QString firstText()const{return list->count()?list->item(0)->text():QString();}
    void removeFirst(){list->setCurrentRow(0);deleteNote();}
};
int main(int argc,char **argv){
    QApplication app(argc,argv);
    if(app.arguments().contains("--self-test")){
        QTemporaryDir dir; if(!dir.isValid())return 1; auto path=dir.filePath("notes.sqlite");
        try {
            bool enabled;
            {
                Window w(path);
                enabled=qEnvironmentVariableIsSet("RPP_NOTES_EXPECT_COUNT")
                    ? qEnvironmentVariableIntValue("RPP_NOTES_EXPECT_COUNT")==1 : w.countVisible();
                w.exercise(enabled);
                app.processEvents();
            }
            {
                Window w(path);
                w.expectCount(enabled,2);
                if(w.firstText()!=QString::fromUtf8("Überarbeitet — 中文 🦀"))return 2;
                w.removeFirst();
                w.expectCount(enabled,1);
                w.removeFirst();
                w.expectCount(enabled,0);
            }
            {Window w(path);w.expectCount(enabled,0);}
            const auto invalidPath=dir.filePath("missing/notes.sqlite");
            Window invalid(invalidPath);
            if(!invalid.showsOpenError())return 4;
            Error error;
            NotesHandle *handle=nullptr;
            if(notes_open(view(invalidPath.toUtf8()),&handle,&error.b)==0 || handle || !error.b.len)return 4;
        }catch(const std::exception &error){qCritical("%s",error.what());return 5;}
        return 0;
    }
    auto storage=QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);QDir().mkpath(storage);
    Window window(storage+"/notes.sqlite");window.show();return app.exec();
}
#include "main.moc"
