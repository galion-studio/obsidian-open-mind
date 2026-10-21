<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/SLAVIK-TODO.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Slavik: 3 koraka prije nego Hermes krene sa spajanjem

**Stanje:** Hermes koordinator je na `swarm/hermes-core` (`45b7980`), provjera spremnosti prolazi 11 od 15, a preostale 4 čekaju tebe.

## 1. HADES admin ključ
Napravi ključ na svom uređaju, a privatni dio nikad ne izlazi s njega. Na server ide samo javni dio.

## 2. ✅ GOTOVO — OpenRouter kredit
~~Nadoplati kredit na OpenRouteru, jer sada vraća grešku 402 (nema kredita). Odobri i sve ostalo što OpenRouter traži da bi nastavio.~~ Dodano je 50 USD kredita.

## 3. Strože obrađivanje zahtjeva
Odobri stavke H1–H3, M1–M3, D1–D4, R1–R4 i C1–C5 u [`docs/approvals/common-migrate-2-strict.md`](https://github.com/galion-studio/obsidian-open-mind/blob/swarm/common-migrate-2/docs/approvals/common-migrate-2-strict.md) na `swarm/common-migrate-2` (`123eab2`). Svaki servis može se kasnije vratiti zasebno.
