// Exercises the shipped QML handlers against actual Qt Widgets dialog windows.
// Unlike a QML rejected() signal test, these clicks/key events hit visible widgets.
#include <QApplication>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFileSystemModel>
#include <QListView>
#include <QPushButton>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickWindow>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>

class Bridge : public QObject {
    Q_OBJECT
public:
    QStringList accepted;
    int calls = 0;
    Q_INVOKABLE void local_import(const QString &, const QVariantList &urls) {
        accepted.clear();
        for (const auto &url : urls) accepted.append(url.toString());
        ++calls;
    }
};
static QFileDialog *visiblePicker() {
    for (auto *widget : QApplication::topLevelWidgets())
        if (auto *dialog = qobject_cast<QFileDialog *>(widget); dialog && dialog->isVisible())
            return dialog;
    return nullptr;
}
static void check(bool ok, const char *message) {
    if (!ok) qFatal("%s", message);
}
int main(int argc, char **argv) {
    QApplication app(argc, argv);
    check(argc == 2, "pass extracted QML path");
    QTemporaryDir temp;
    QStringList paths;
    for (const auto &name : {"a.flac", "b.flac", "c.flac"}) {
        const auto path = temp.filePath(name);
        QFile file(path); check(file.open(QIODevice::WriteOnly), "create disposable file");
        file.write("fixture"); paths.append(path);
    }
    Bridge bridge;
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty("diagnostic", &bridge);
    engine.load(QUrl::fromLocalFile(argv[1]));
    check(!engine.rootObjects().isEmpty(), "load shipped picker QML");
    auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
    QTest::qWait(80);
    for (const auto &name : {"localFilesPicker", "localFolderPicker"}) {
        auto *picker = window->findChild<QObject *>(name);
        check(picker, "find picker");
        check(picker->property("parentWindow").value<QObject *>() == window, "shared owning window");
        for (int action = 0; action < 3; ++action) {
            check(QMetaObject::invokeMethod(picker, "openFresh"), "open fresh picker");
            QTest::qWait(80);
            auto *dialog = visiblePicker();
            check(dialog, "actual dialog QWidget is visible");
            check(dialog->windowModality() == Qt::WindowModal, "same window modality");
            check(dialog->windowHandle()->transientParent() == window, "actual transient parent is application window");
            const auto staleSelection = QString(name) == "localFilesPicker" ? paths.first() : temp.path();
            dialog->setDirectory(temp.path());
            dialog->selectFile(staleSelection);
            if (action == 0) {
                auto *buttons = dialog->findChild<QDialogButtonBox *>();
                check(buttons && buttons->button(QDialogButtonBox::Cancel), "native Cancel button exists");
                QTest::mouseClick(buttons->button(QDialogButtonBox::Cancel), Qt::LeftButton);
            } else if (action == 1) {
                QTest::keyClick(dialog, Qt::Key_Escape);
            } else {
                dialog->close(); // Actual window-close event, not the QML wrapper.
            }
            QTest::qWait(80);
            check(!visiblePicker(), "Cancel/Escape/window-close must hide actual dialog window");
            check(!picker->property("visible").toBool(), "QML lifecycle agrees with actual window");
            check(bridge.calls == 0, "cancellation never imports");
            check(QMetaObject::invokeMethod(picker, "openFresh"), "reopen picker");
            QTest::qWait(80);
            dialog = visiblePicker();
            check(dialog, "reopened actual dialog visible");
            check(!dialog->selectedFiles().contains(staleSelection), "fresh window has no stale selection");
            dialog->close(); QTest::qWait(80);
        }
    }
    auto *picker = window->findChild<QObject *>("localFilesPicker");
    picker->setProperty("folder", QUrl::fromLocalFile(temp.path()));
    QMetaObject::invokeMethod(picker, "openFresh"); QTest::qWait(120);
    auto *dialog = visiblePicker();
    check(dialog && dialog->fileMode() == QFileDialog::ExistingFiles, "Qt native multi-file mode");
    dialog->setDirectory(temp.path()); dialog->setViewMode(QFileDialog::List);
    QTest::qWait(120);
    auto *view = dialog->findChild<QListView *>("listView");
    check(view, "standard Qt file list exists");
    auto *model = qobject_cast<QFileSystemModel *>(view->model());
    check(model, "standard filesystem model");
    auto click = [&](int index, Qt::KeyboardModifiers modifiers) {
        auto item = model->index(paths[index]);
        check(item.isValid(), "file model index");
        view->scrollTo(item); QTest::qWait(20);
        QTest::mouseClick(view->viewport(), Qt::LeftButton, modifiers, view->visualRect(item).center());
        QTest::qWait(20);
    };
    click(0, Qt::NoModifier); click(1, Qt::ControlModifier);
    check(view->selectionModel()->selectedRows().size() == 2, "Ctrl selects individual files");
    click(0, Qt::ControlModifier);
    check(view->selectionModel()->selectedRows().size() == 1, "Ctrl deselects an individual file");
    click(0, Qt::NoModifier); click(2, Qt::ShiftModifier);
    check(view->selectionModel()->selectedRows().size() == 3, "Shift selects a range");
    auto *buttons = dialog->findChild<QDialogButtonBox *>();
    check(buttons && buttons->button(QDialogButtonBox::Open), "native Open button exists");
    QTest::mouseClick(buttons->button(QDialogButtonBox::Open), Qt::LeftButton);
    QTest::qWait(80);
    check(!visiblePicker(), "accepted dialog disappears");
    check(bridge.calls == 1 && bridge.accepted.size() == 3, "all selected URLs reach shipped import handler once");
    for (const auto &path : paths)
        check(bridge.accepted.contains(QUrl::fromLocalFile(path).toString()), "selected URL retained");
    qInfo("Actual Qt dialog Cancel/Escape/close/reopen and Ctrl/Shift multi-selection passed");
}
#include "native_picker_lifecycle.moc"
