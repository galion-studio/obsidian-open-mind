# t376u/t379u: Hermes always-on CPU setup, sandbox test (2026-10-29, Europe/Zagreb)

Not applied live. On the VPS, only reads were done, plus one test ask through river-flash (tiny cost, logged spend unchanged).

Sandbox: box, systemd 257 as PID 1 in new pid/mnt/net/uts/ipc/cgroup namespaces, chroot /workspace/swarm/sandbox-t376u/root.
cgroup /sys/fs/cgroup/t376u/payload. Live binary hermes-gateway-b7fbbc78 (sha256 c5432a88…, same as on the VPS).
Live unit and the 4 live drop-ins are byte copies. The proposed 95-cpu-always.conf is added. 99-sandbox-mock.conf (--mock, dummy key) is sandbox only.
Scripts: /workspace/swarm/sandbox-t376u/build-root.sh, /workspace/swarm/sandbox-t376u/boot.sh. Files: /workspace/swarm/sandbox-t376u/files/.

## Results
- Boot: hermes-rs enabled, active at boot, /health 200, mock chat 200.
- kill -9: restarted in about 3 s. kill -TERM: restarted. 8 rapid kills: still active (StartLimitIntervalSec=0), NRestarts=10.
- Control run with the live settings only (95 removed): after SIGTERM the service stays **inactive** (Restart=on-failure).
- Exit 2 (config error): not restarted (RestartPreventExitStatus=2), so no crash loop.
- Reboot (reboot.target, then boot again): clean stop, then active at the next boot with no manual start (3 boots).
- CPU only: the service /dev has only null, zero, full, random, urandom, tty, pts, shm. 0 GPU libraries mapped. 4 threads (hermes-gateway-, nerve-push, nerve-heartbeat, hermes-accept).
- Security: /opt/galion/openmind cannot be reached (InaccessiblePaths). Writes to /opt/galion/{rust,hermes,openmind}, /etc, /usr and /root fail with EROFS. Only /var/lib/hermes-rs can be written. 0 HERMES_RS_CODING_* variables. NoNewPrivs=1, Seccomp=2. systemd-analyze exposure 8.1.
- Landlock: the box kernel has it (ABI 6). The VPS LXC does not (landlock_create_ruleset gives EOPNOTSUPP, and lsm=lockdown,capability,yama,apparmor). The coding runtime stays OFF.
