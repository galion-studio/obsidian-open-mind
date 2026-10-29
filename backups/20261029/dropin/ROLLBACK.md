# hermes-rs drop-in 95-cpu-always.conf (live since 13:41:46 CET 2026-10-29)

Live file: /etc/systemd/system/hermes-rs.service.d/95-cpu-always.conf
sha256 prefix 5014b75c95c9 (re-read at 14:20 CET, read-only). A copy is in this folder.
Backup of the drop-in dir before the change: /var/backups/hermes-rs.service.d.2026-10-29-1241 (copied to previous-dropins/).

Rollback (as root on the VPS):

    rm /etc/systemd/system/hermes-rs.service.d/95-cpu-always.conf
    systemctl daemon-reload
    systemctl restart hermes-rs
    systemctl is-active hermes-rs
