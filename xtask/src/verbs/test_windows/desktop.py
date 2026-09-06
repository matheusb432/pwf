import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

bundle, container, report = sys.argv[1:]
bundle = pathlib.Path(bundle)
complete = bundle / "output/complete.json"
credentials_path = bundle / "connection/login.json"
deadline = time.monotonic() + 90
while not credentials_path.is_file():
    if complete.is_file():
        sys.exit(0)
    if time.monotonic() >= deadline:
        raise RuntimeError("Windows did not request a temporary desktop session")
    time.sleep(0.5)

credentials = json.loads(credentials_path.read_text(encoding="utf-8-sig"))
credentials_path.unlink()
ports = subprocess.check_output(
    ["docker", "port", container, "3389/tcp"], text=True, timeout=10
)
port = int(ports.splitlines()[0].rsplit(":", 1)[1])
with tempfile.TemporaryDirectory(prefix="pwf-rdp-") as temporary:
    temporary = pathlib.Path(temporary)
    environment = dict(os.environ)
    environment["DBUS_SESSION_BUS_ADDRESS"] = "unix:path=/nonexistent/pwf-session-bus"
    environment["GIO_USE_VFS"] = "local"
    environment["GDK_BACKEND"] = "x11"
    for name in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]:
        environment[name] = str(temporary / name.lower())
    profile = temporary / "smoke.remmina"
    profile.write_text(
        "[remmina]\nname=PWF smoke\nprotocol=RDP\ncert_ignore=1\n"
        f"server=127.0.0.1:{port}\nsecurity=nla\n"
        f"username={credentials['username']}\ndomain={credentials['domain']}\n"
        "disableclipboard=1\nresolution_width=1024\nresolution_height=768\n"
    )
    profile.chmod(0o600)
    log_path = temporary / "desktop.log"
    bus = temporary / "bus.conf"
    bus.write_text(
        '<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>'
        '<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/>'
        '<allow own="*"/></policy></busconfig>'
    )
    with log_path.open("w+") as log:
        subprocess.run(
            ["remmina", "--update-profile", str(profile), "--set-option", "password"],
            input=credentials["password"] + "\n", text=True, env=environment,
            stdout=log, stderr=log, check=True, timeout=20,
        )
        desktop = subprocess.Popen(
            ["dbus-run-session", "--config-file", str(bus), "--", "remmina",
             "--no-tray-icon", "--disable-news", "--disable-stats", "-c", str(profile)],
            env=environment, stdout=log, stderr=log,
        )
        try:
            deadline = time.monotonic() + 360
            while not complete.is_file():
                if time.monotonic() >= deadline:
                    raise RuntimeError("Windows desktop tests did not finish within six minutes")
                time.sleep(0.5)
        finally:
            desktop.terminate()
            try:
                desktop.wait(timeout=10)
            except subprocess.TimeoutExpired:
                desktop.kill()
                desktop.wait(timeout=5)
            log.seek(0)
            output = log.read().replace(credentials["password"], "[temporary password]")
            output += f"\nDesktop client exit: {desktop.returncode}\n"
            (pathlib.Path(report) / "desktop.log").write_text(output)
