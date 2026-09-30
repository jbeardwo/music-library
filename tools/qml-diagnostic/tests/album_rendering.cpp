// Inspect the shipped shared AlbumArt renderer using cached files only.
#include <QApplication>
#include <QCryptographicHash>
#include <QDateTime>
#include <QFile>
#include <QImage>
#include <QPainter>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickItem>
#include <QQuickWindow>
#include <QTemporaryDir>
#include <QTest>
class Bridge : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap artwork_snapshot MEMBER images CONSTANT)
    Q_PROPERTY(QVariantList samples MEMBER samples CONSTANT)
public:
    QVariantMap images;
    QVariantList samples;
    int requests=0;
    Q_INVOKABLE void artwork_batch(const QVariantList &) { ++requests; }
    Q_INVOKABLE void artwork_retry(const QString &) { qFatal("cached image failed to load"); }
};
static void check(bool ok,const char *message) { if (!ok) qFatal("%s",message); }
static QList<QQuickItem *> covers(QQuickItem *item) {
    QList<QQuickItem *> result;
    if (item->objectName()=="coverImage") result.append(item);
    for (auto *child:item->childItems()) result.append(covers(child));
    return result;
}
static QByteArray hash(const QString &path) {
    QFile file(path);check(file.open(QIODevice::ReadOnly),"read test image");
    return QCryptographicHash::hash(file.readAll(),QCryptographicHash::Sha256);
}
int main(int argc,char **argv) {
    QApplication app(argc,argv);
    check(argc>=3,"pass QML and screenshot paths, then representative caches");
    QTemporaryDir temp;
    QStringList paths;
    for(int i=3;i<argc;++i) paths.append(QString::fromLocal8Bit(argv[i]));
    QImage lines(500,500,QImage::Format_RGB32);lines.fill(Qt::white);
    QPainter painter(&lines);painter.setPen(Qt::black);
    for(int i=0;i<500;i+=3) painter.drawLine(i,80,499-i,499);
    painter.setFont(QFont("Sans",12));painter.drawText(20,30,"Fine text and high-contrast line art");painter.end();
    auto linePath=temp.filePath("lines.png");lines.save(linePath);paths.append(linePath);
    QImage small(24,16,QImage::Format_RGB32);small.fill(Qt::magenta);
    auto smallPath=temp.filePath("small.png");small.save(smallPath);paths.append(smallPath);
    auto widePath=temp.filePath("wide.png");lines.copy(0,0,500,300).save(widePath);paths.append(widePath);
    Bridge bridge;
    QList<QByteArray> hashes;QList<QDateTime> dates;
    for(int i=0;i<paths.size();++i) {
        const auto key=QString("cover%1").arg(i);
        bridge.images.insert(key,QUrl::fromLocalFile(paths[i]).toString());
        bridge.samples.append(QVariantMap{{"key",key},{"url",QUrl::fromLocalFile(paths[i])},{"name",QFileInfo(paths[i]).fileName()}});
        hashes.append(hash(paths[i]));dates.append(QFileInfo(paths[i]).lastModified());
        qInfo()<<"source"<<paths[i]<<QImage(paths[i]).size();
    }
    QQmlApplicationEngine engine;engine.rootContext()->setContextProperty("diagnostic",&bridge);
    engine.load(QUrl::fromLocalFile(argv[1]));check(!engine.rootObjects().isEmpty(),"load renderer");
    auto *window=qobject_cast<QQuickWindow*>(engine.rootObjects().first());
    QTest::qWait(500);
    const auto images=covers(window->contentItem());
    check(images.size()==paths.size()*2,"every shared renderer inspected");
    for(auto *image:images) {
        check(image->property("status").toInt()==1,"all shared images loaded");
        const auto size=image->property("sourceSize").toSize();
        check(size.width()<=120*window->devicePixelRatio(),"requested textures bounded to displayed size");
        check(image->implicitWidth()<=size.width() && image->implicitHeight()<=size.height(),"actual decoded texture bounded in both axes");
        const auto path=image->property("source").toUrl().toLocalFile();
        const auto original=QImage(path).size();
        check(image->implicitWidth()<=original.width() && image->implicitHeight()<=original.height(),"small originals must not be upscaled");
        check(image->width()<=image->parentItem()->width() && image->height()<=image->parentItem()->height(),"image fitted inside square");
        qInfo()<<"decoded"<<image->property("source")<<size<<"implicit"<<image->implicitWidth()<<image->implicitHeight();
    }
    check(window->grabWindow().save(argv[2]),"save actual QML render for visual inspection");
    const auto requests=bridge.requests;
    window->setWidth(window->width()+100);QTest::qWait(100);
    check(bridge.requests==requests,"layout resize makes no artwork requests");
    for(int i=0;i<paths.size();++i)check(hash(paths[i])==hashes[i] && QFileInfo(paths[i]).lastModified()==dates[i],"render must not rewrite cache");
    qInfo("Shared image render, bounded textures, unchanged caches and no resize requests passed");
}
#include "album_rendering.moc"
