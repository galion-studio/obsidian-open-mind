#!/usr/bin/env bash
# Connect to / run a command on Slavik's VPS using config.env + ~/.ssh/id_ed25519_vps.
# Usage:
#   vps-ssh.sh                 interactive shell
#   vps-ssh.sh --check         non-interactive connectivity + identity test (read-only)
#   vps-ssh.sh -- <command>    run one command (you are responsible for it being read-only)
#   vps-ssh.sh --print-config  show effective settings, no connection
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"

case "${1:-}" in
  --print-config)
    echo "host=$VPS_HOST user=$VPS_USER port=$VPS_PORT alias=$VPS_ALIAS key=$VPS_KEY"
    echo "pubkey to install on VPS (~$VPS_USER/.ssh/authorized_keys):"
    cat "${VPS_KEY}.pub" 2>/dev/null || warn "no ${VPS_KEY}.pub"
    ;;
  --check)
    vps_ssh 'echo "connected: $(whoami)@$(hostname) | $(uname -srm)"; . /etc/os-release 2>/dev/null && echo "os: $PRETTY_NAME"; echo "uptime: $(uptime)"; if sudo -n true 2>/dev/null; then echo "sudo: passwordless OK"; else echo "sudo: needs password or not allowed"; fi'
    ;;
  --)
    shift; [ $# -gt 0 ] || die "no command given after --"
    vps_ssh "$@"
    ;;
  "")
    require_target; build_ssh_opts
    # interactive: drop BatchMode so prompts (host key, etc.) work
    exec ssh "${VPS_SSH_OPTS[@]}" -o BatchMode=no -t "$VPS_ALIAS"
    ;;
  -h|--help) sed -n '2,8p' "$0" ;;
  *) die "unknown option: $1 (see --help)" ;;
esac
