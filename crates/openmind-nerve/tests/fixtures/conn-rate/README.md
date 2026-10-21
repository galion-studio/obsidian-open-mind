<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# conn-rate fixture (T2.7)

Synthesised `/proc/net/tcp` + `/proc/net/tcp6` tables (loopback only, no real addresses), three reads 30 s apart.
Generated 2026-10-20 23:30 CEST (+02:00) with a bash `printf` loop (no Python).

| file | :8420 (memory-gateway) | :8002 |
|---|---|---|
| tcp-0.txt | LISTEN, 300 TIME-WAIT (ports 10000+), 5 ESTAB (both loopback ends) | LISTEN, 20 TIME-WAIT (both ends) |
| tcp-1.txt | LISTEN, 300 *other* TIME-WAIT (20000+), same 5 ESTAB | LISTEN, 30 other TIME-WAIT |
| tcp-2.txt | = tcp-1.txt | = tcp-1.txt |

Expected (`connrate::tests::replay_fixture`): cycle 0 baseline, no finding; cycle 1 `conn-rate-8420` high
(300 new / 30 s = 10/s, while the B41 ESTAB+TIME-WAIT growth is 0/s) and `conn-rate-8002` warn (30 / 30 s = 1/s);
cycle 2 nothing new, both clear.
