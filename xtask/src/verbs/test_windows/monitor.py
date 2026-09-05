import re
import socket
import ipaddress
import pathlib
import subprocess
import sys
import time

run_id = sys.argv[1]
if not re.fullmatch(r"pwf-smoke-[A-Za-z0-9]+", run_id):
    raise ValueError("invalid smoke-test directory name")

deadline = time.monotonic() + 300
while True:
    try:
        leases = pathlib.Path('/var/lib/misc/dnsmasq.leases').read_text().splitlines()
        if len(leases) != 1:
            raise ValueError('expected one dockur guest DHCP lease')
        guest = str(ipaddress.IPv4Address(leases[0].split()[2]))
        with socket.create_connection((guest, 3389), timeout=1):
            pass
        subprocess.run(['smbcontrol', 'smbd', 'close-share', 'Data'], check=True, timeout=5)
        break
    except (OSError, ValueError, IndexError, subprocess.SubprocessError):
        if time.monotonic() >= deadline:
            raise RuntimeError('Windows guest and shared directory did not become ready within five minutes')
        time.sleep(1)

with socket.socket(socket.AF_UNIX) as monitor:
    monitor.settimeout(5)
    monitor.connect("/run/shm/monitor.sock")

    def response():
        result = b""
        while not result.endswith(b"(qemu) "):
            chunk = monitor.recv(65536)
            if not chunk:
                raise RuntimeError("QEMU monitor disconnected")
            result += chunk
            if len(result) > 1048576:
                raise RuntimeError("QEMU monitor response exceeded one MiB")
        return result

    response()

    def send(command):
        monitor.sendall((command + "\n").encode("ascii"))
        result = response()
        if b"unknown command" in result.lower() or b"invalid" in result.lower():
            raise RuntimeError("QEMU rejected command: " + command)
        time.sleep(0.12)

    # The existing VM must have reached its unlocked desktop before keyboard input.
    time.sleep(10)
    send("sendkey meta_l-r")
    time.sleep(1)
    send("sendkey ctrl-a")
    command = rf"cmd /c \\host.lan\Data\{run_id}\launch.cmd"
    keys = {" ": "spc", "/": "slash", "\\": "backslash", ".": "dot", "-": "minus"}
    for character in command:
        key = keys.get(character, character.lower())
        if character.isupper():
            key = "shift-" + key
        send("sendkey " + key + " 40")
    send("sendkey ret")
