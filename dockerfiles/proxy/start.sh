#!/bin/bash
set -euo pipefail

ip rule add fwmark 0x1/0x1 lookup 100 2>/dev/null || true
ip route add local default dev lo table 100 2>/dev/null || true

ip -6 rule add fwmark 0x1 lookup 106 2>/dev/null || true
ip -6 route add local ::/0 dev lo table 106 2>/dev/null || true

/app/redirect_pkg_handler &
sing-box run -c /etc/sing-box/config.json &

wait
