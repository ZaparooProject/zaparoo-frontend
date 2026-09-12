// Runs the real UI test binary with a hostile inherited Core endpoint. The
// endpoint is a test-owned TCP tripwire, never a real Core or a fixed port.
#include <QCoreApplication>
#include <QDebug>
#include <QProcess>
#include <QProcessEnvironment>
#include <QScopeGuard>
#include <QSocketNotifier>
#include <QTimer>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    const QStringList args = QCoreApplication::arguments();
    if (args.size() != 3)
    {
        qCritical("Expected UI test executable and QML test directory");
        return 1;
    }

    const int listener = socket(AF_INET, SOCK_STREAM, 0);
    if (listener < 0)
    {
        qCritical("Cannot create the test-owned Core tripwire");
        return 1;
    }
    const auto closeSocket = qScopeGuard([listener] { close(listener); });
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    socklen_t addressLength = sizeof(address);
    // Native socket APIs require their generic address pointer type.
    // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
    auto* socketAddress = reinterpret_cast<sockaddr*>(&address);
    if (fcntl(listener, F_SETFD, FD_CLOEXEC) < 0 ||
        bind(listener, socketAddress, sizeof(address)) < 0 || listen(listener, 1) < 0 ||
        getsockname(listener, socketAddress, &addressLength) < 0)
    {
        qCritical("Cannot bind the test-owned Core tripwire");
        return 1;
    }

    QProcess child;
    auto environment = QProcessEnvironment::systemEnvironment();
    environment.insert(QStringLiteral("ZAPAROO_CORE_ENDPOINT"),
                       QStringLiteral("ws://127.0.0.1:%1/api/v0.1").arg(ntohs(address.sin_port)));
    child.setProcessEnvironment(environment);
    child.setProgram(args.at(1));
    child.setArguments(
        {QStringLiteral("-input"), args.at(2), QStringLiteral("-platform"),
         QStringLiteral("offscreen"),
         QStringLiteral(
             "UiNavigation::test_resume_with_cached_row_clears_the_cue_in_the_same_tick")});
    child.setProcessChannelMode(QProcess::ForwardedChannels);

    bool contactedCore = false;
    bool timedOut = false;
    QSocketNotifier tripwire(listener, QSocketNotifier::Read);
    QObject::connect(&tripwire, &QSocketNotifier::activated, &app,
                     [&]
                     {
                         contactedCore = true;
                         tripwire.setEnabled(false);
                         child.kill();
                     });
    QTimer deadline;
    deadline.setSingleShot(true);
    QObject::connect(&deadline, &QTimer::timeout, &app,
                     [&]
                     {
                         timedOut = true;
                         child.kill();
                     });
    QObject::connect(&child, &QProcess::finished, &app, &QCoreApplication::quit);
    QObject::connect(&child, &QProcess::errorOccurred, &app, &QCoreApplication::quit);
    child.start();
    deadline.start(20000);
    QCoreApplication::exec();
    if (child.state() != QProcess::NotRunning)
    {
        child.kill();
        child.waitForFinished(5000);
    }
    // A child exit must not hide a connection queued before the notifier ran.
    pollfd pending{listener, POLLIN, 0};
    contactedCore = contactedCore || poll(&pending, 1, 0) > 0;
    if (contactedCore || timedOut || child.exitStatus() != QProcess::NormalExit ||
        child.exitCode() != 0 || child.error() == QProcess::FailedToStart)
    {
        qCritical() << "UI isolation failed: contacted Core=" << contactedCore
                    << "timed out=" << timedOut;
        return 1;
    }
    return 0;
}
