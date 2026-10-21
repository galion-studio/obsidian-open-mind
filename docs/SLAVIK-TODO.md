# Slavik: 3 koraka prije nego Hermes krene sa spajanjem

**Stanje:** Hermes koordinator je na `swarm/hermes-core` (`45b7980`), provjera spremnosti prolazi 11 od 15, a preostale 4 čekaju tebe.

## 1. HADES admin ključ
Napravi ključ na svom uređaju, a privatni dio nikad ne izlazi s njega. Na server ide samo javni dio.

## 2. ✅ GOTOVO — OpenRouter kredit
~~Nadoplati kredit na OpenRouteru, jer sada vraća grešku 402 (nema kredita). Odobri i sve ostalo što OpenRouter traži da bi nastavio.~~ Dodano je 50 USD kredita.

## 3. Strože obrađivanje zahtjeva
Odobri stavke H1–H3, M1–M3, D1–D4, R1–R4 i C1–C5 u [`docs/approvals/common-migrate-2-strict.md`](https://github.com/galion-studio/obsidian-open-mind/blob/swarm/common-migrate-2/docs/approvals/common-migrate-2-strict.md) na `swarm/common-migrate-2` (`123eab2`). Svaki servis može se kasnije vratiti zasebno.

---

# Otvorene odluke iz slotova 195–244 (stanje 22. 10. 2026. 02:00 CEST, +02:00)

Izvor: `/workspace/swarm/CALL-RECORD.md` i dokumenti na granama slotova. Uz svaki slot stoji grana i SHA, a „LOKALNO“ znači da grana još nije na GitHubu.
✅ znači da je odluka već riješena tvojim odobrenjima: živi prelasci i rad na VPS-u (20. 10., auto-prihvat), spajanje u main svega što prođe testove i sandbox (21. 10. 23:06), stvarne radnje cortexa (21. 10. 22:50). Riješeno znači odobreno, a agenti to još izvode.

## Ops i VPS

**195 vps-preflight-scripts** — `swarm/vps-preflight-scripts` `7733aa93` (LOKALNO)
- D-195-1: samo Rust alat ili i `.sh` omotači? Pravila kažu samo Rust.
- D-195-2: prihvaćamo li cloudflared NO-GO (20/21) dok su stari zatvoreni portovi tunela?
- D-195-3: cloudflared binarna datoteka ima prava 775, treba li ih spustiti na 755?
- D-195-4: smije li preflight prihvatiti poznato loše stanje galion-one (503 „loading“)?
- ✅ D-195-5: instalacija alata na VPS — pokriveno odobrenjem za rad na VPS-u.
- D-195-6: treba li galion-deploy odbiti deploy kad preflight kaže NO-GO?

**203 docs-cutover-missing** — `swarm/docs-cutover-missing` `878485d1` (LOKALNO)
- D-203-1..5: link-watch-rs na serveru nema dokazan izvor, tanga-serve izvor je izvan gita, a stari `rollback.sh` za hermes ne radi. Treba odlučiti što od toga popraviti, a što samo zabilježiti.

**210 vps-rollback-table** — `swarm/vps-rollback-table` `7a1990e5` (LOKALNO)
- D-210-1: prihvaćaš li zamrzavanja F1 i F2 kako su napisana i do kada vrijedi F2?
- ✅ D-210-2: smije li agent bez novog odobrenja pokrenuti siguran rollback (R19, R22, R21) kad servis padne? Pokriveno auto-prihvatom za VPS rad.
- D-210-3: tko piše naknadne zapise za R21 i R22? Prijedlog: agenti koji su ih napravili.
- ✅ D-210-4: proba rollbacka drop-ina na mirnom servisu (openmind-memory R12) — pokriveno odobrenjem za VPS rad.
- ✅ D-210-5: `chmod 600` na cloudflared backup 150844 — odobreno kao VPS rad, još nije izvedeno.

**232 vps-live-table** — `swarm/vps-live-table` `a8507c18`
- D-232-1: samo openmind-memory ima ugrađen commit, pa treba li galion-deploy tražiti oznaku commita?
- D-232-2: gaia-hermes-handoff radi s c5cae88, a cortex s bd6fbe6, treba li ih uskladiti?
- D-232-3: tablici rollbacka nedostaju retci R23–R26 (drop-ini 85/86/87 i handoff), tko ih piše?
- D-232-4: koliko dugo čuvamo stare binarne datoteke cortexa i nerve?

**214 gaia-lan-fallback-test** — `swarm/gaia-lan-fallback-test` `b8358fbc` (LOKALNO)
- F1–F4: zaglavljeni GPU pružatelj kasnije izvrši preusmjereni zahtjev (duplikat), a POSEIDON drži mrtav GPU dostupnim do ponovnog spajanja. Treba odlučiti koje od četiri nalaza popraviti prije LAN-a.

## Nerve

**200 nerve-t23-specs** — `swarm/nerve-t23-specs` `7c32ec3f` (LOKALNO)
- D-200-1: kad se hades-gate instalira, treba li njegova odsutnost postati greška?
- D-200-2: backup star 13 h, je li to prihvatljivo ili treba upozorenje?
- ✅ D-200-3: promjena živog rezultata nakon deploya — pokriveno odobrenjem živih prelazaka.
- D-200-4: spec za cortex i Gaia funkcija za port 8674.

**212 nerve-parser-fuzz** — `swarm/nerve-parser-fuzz` `5ad5d997`
- D-212-1: stroža granica za veličinu zaglavlja zahtjeva.
- D-212-2: stog od 512 KiB je li dovoljan?
- D-212-3: uključiti serde_json `float_roundtrip` ili prihvatiti pomak od 1 ULP?
- D-212-4: koje popustljivosti `read_req` zadržati (kratko tijelo, `+5` Content-Length, dupli CL, sporo kapanje)?
- D-212-5: raspored spajanja s t52 `mod.rs`.

**217 nerve-cross-target** — `swarm/nerve-cross-target` `71f895b2`
- D-217-1: zaključati recept za build nerve i izričito postaviti `preserve_order`?
- D-217-2: isporučujemo li nerve i za aarch64?

**223 nerve-t41-statedir** — `swarm/nerve-t41-statedir` `cd359d6`
- D-T41-1: oštećen `reflex.json` zaustavi restarte na 1 h (fail closed) ili ga ignorira kao prije?
- D-T41-2: vratiti prozor deduplikacije upozorenja iz `alerts.json` nakon restarta (manje ponovnih poziva)?
- D-T41-3: oštećen zapis potrošnje blokira plaćenu eskalaciju dok ga operater ne popravi.
- D-T41-4: `dry_1h` budžeti ostaju samo u memoriji.
- D-T41-5: `/stats.llm_usd` prikazuje potrošnju od starta procesa, a ograničenje koristi spremljeni zbroj.

**226 nerve-t24-coverage-gate** — `swarm/nerve-t24-coverage-gate` `457bc875`
- D-T24-1: CI sad ima 19 novih grešaka, zadržati grešku ili privremeno upozorenje?
- D-T24-2: razvrstati i spojiti 19 rupa, prvo živi servisi (queue, one, health, desk, link-watch, tuner).
- D-T24-3: računa li se guardian-tick preko biblioteke openmind-nerve kao pokrivenost?
- D-T24-4: tranzitivna pokrivenost za benchmarke.

**233 nerve-t61-incidents** — `swarm/nerve-t61-incidents` `416521df` (LOKALNO)
- ✅ D-233-1: popraviti lažni DDoS (brojač od 900 s ubačen u kantu od 10 s) i ponovno deployati nerve — pokriveno odobrenjem živih prelazaka.
- D-233-2: mrtvi originalni portovi tunela, pokrenuti 5–8 servisa ili maknuti ingress pravila i DNS? DNS promjene trebaju tvoje izričito odobrenje.
- D-233-3: port 18800 (ssh prosljeđivanje), prihvatiti kao poznat ili zatvoriti sesiju? Slot 238 ga namjerno nije dodao na popis.
- D-233-4: Hermes predaja piše `no-channel-configured`, pa Hermes ne može nikoga obavijestiti; treba odabrati kanal.
- D-233-5: prag sporosti za hermes-rs, promjena koda ili `NERVE_SLOW_MS` u unitu?

**238 nerve-own-services** — `swarm/nerve-own-services` `9479050c` (LOKALNO)
- ✅ D-T44-2 (dio): 8672, 8674 i 19120 dodani su kao vlastiti servisi nerve; ide u main po pravilu spajanja.

**241 no-openrouter-mergefix** — `swarm/no-openrouter-mergefix` `4b835a61` (LOKALNO, namjerno)
- D-241-1: nerve ostaje bez izravnog HTTP-a prema Hermesu (predaja samo preko cortexa) ili ga dopustiti?
- D-241-2: nerve ima izravnog LLM klijenta (ne OpenRouter, isključen po defaultu), zadržati ga ili ukloniti da `LLM_ENABLED=false` bude apsolutan?

## Cortex

**207 cortex-t31-calibrate** — `swarm/cortex-t31-calibrate` `b1c3efda` (LOKALNO)
- D-T31-1: naleti na mrtve origine pale upozorenje edge-rate, prihvatiti, spojiti u edge incident ili maknuti mrtve rute?
- D-T31-2: prag za `preauth_other`.
- D-T31-3: koliko često ponovno kalibrirati.
- D-T31-4: uključiti `NERVE_SSHD` na živom sustavu?

**222 cortex-t44-e2e** — `swarm/cortex-t44-e2e` `78936986` (LOKALNO)
- ✅ D-T44-1: stvarni izvršitelj popravaka — odobreno 21. 10. 22:50 (stvarne radnje cortexa).
- D-T44-2: 8672, 8674 i 19120 riješeni u slotu 238; 18800 ostaje otvoren (vidi D-233-3).
- D-T44-3: zapisivati popravke (heal) uz incidente?
- ✅ D-T44-4: deploy T4.2 — pokriveno odobrenjem živih prelazaka.

**227 cortex-t51-chaos** — `swarm/cortex-t51-chaos` `e9822109` (LOKALNO)
- F1/F2/F6 vrijede i na živom cortexu (deduplikacija nerve od 600 s skriva petlje rušenja, cortex je slijep nakon restarta nerve). Treba potvrditi da se popravci spoje u cortex-hermes-boss i deployaju.

**235 cortex-flap-dedupe** — `swarm/cortex-flap-dedupe` `a06bdbd3` (LOKALNO)
- D-235-1: interval provjere 30 s.
- D-235-2: najviše 2 akcije po servisu i 6 ukupno.
- D-235-3: provjeriti ograničenja nerve prije dry-run odgovora.
- ✅ D-235-4: deploy — pokriveno odobrenjem živih prelazaka (slot 245 to spaja).

**236 cortex-api-load** — `swarm/cortex-api-load` `5e0adbab`, `swarm/cortex-api-load-live` `50f5d43` (LOKALNO)
- ✅ D-236-1: deploy — pokriveno odobrenjem živih prelazaka.
- D-236-2: izvesti I/O prema nerve izvan zaključavanja (slot 247 radi na tome).
- D-236-3: `evict_idle` i za ostale servise?
- D-236-4: objava usred serije (mid-batch publish).
- D-236-5: izbacivanje sporih tijela zahtjeva.

**237 cortex-incident-backfill** — `swarm/cortex-incident-backfill` `1754ff27` (LOKALNO)
- ✅ D-237-1: deploy (spoj u cortex-live-fixes) — pokriveno odobrenjem živih prelazaka.
- D-237-2: koliko dugo drže „ljepljivi“ incidenti (nerve nikad ne objavi oporavak postfixa i ssh-a)?
- D-237-3: slati upozorenja za incidente vraćene nakon svakog starta?
- D-237-4: jednokratna tišina 1800 s nakon starta.

## Gaia i POSEIDON

**199 gaia-fault-followups** — `swarm/gaia-fault-followups` `e806777f`
- D-199-1: testni portovi 19000–20999 bez živih portova.
- D-199-2: ručni popis `LIVE_PORTS` ili provjera slušatelja u stvarnom vremenu?
- D-199-3: nezabilježen restart računati kao restart i grešku.
- D-FI2, D-FI3, D-FI6: podjela provjere i potrošnje tokena i zaključavanje petlje po servisu.

**205 poseidon-auth** — `swarm/poseidon-auth` `d10cb098`
- D-205-1: zajednička tajna ili potpisani zahtjevi preko hades-gate Ed25519?
- D-205-2: `/healthz` iza tokena za LAN ili otvoren?
- D-205-3: običan HTTP na LAN-u otkriva token, pa samo pouzdana mreža ili tunel, ili TLS?
- D-205-4: gdje stoji token, tko ga posjeduje i kako se rotira.
- D-205-5: tokeni po radniku kasnije.

**206 poseidon-crash-soak** — `swarm/poseidon-crash-soak` `b71a9099` (+ `swarm/poseidon-refusal-persist` `eec2d99`)
- D-206-1: glibc RSS raste, prijeći na samo musl ili ograničiti arene?
- D-206-2: blokada budžeta po agentu vrijedi samo unutar procesa.
- D-206-3: potpun redak s lošim CRC-om na kraju se odreže umjesto odbije.
- ✅ D-206-4: spoj i ponovni deploy POSEIDON-a — pokriveno odobrenjima.
- ✅ D-206-5: redoslijed spajanja s eec2d99 — agenti sad spajaju sami.

**218 gaia-poseidon-handle** — `swarm/gaia-poseidon-handle` `dd391914`
- D-218-1: kada Gaia na VPS-u pokazuje na živi red 127.0.0.1:19120?
- D-218-2: zahtjev izvan ugovora označiti kao „refused“ ili „rejected“?
- D-218-3: teme događaja na GAIA sabirnici.

**221 gaia-queue-fuzz** — `swarm/gaia-queue-fuzz` `451db69d` (LOKALNO)
- D-221-1: `MAX_COST` = 2^40 jedinica po pokušaju.
- D-221-2: drugi zapis za isti živi posao se ignorira (prvi pobjeđuje).
- D-221-3: posao agenta `*` u logu, kako ga tretirati.
- D-221-4: `next_token` staje na u64::MAX.
- ✅ D-221-5: ponovni deploy živog reda — pokriveno odobrenjem živih prelazaka.
- D-221-6: fuzzati parser auth zaglavlja iz slota 205 kad se spoji.

**224 gaia-live-classify** — `swarm/gaia-live-classify` `055fe91` (LOKALNO)
- D-224-1: gaia-cortex staviti pod MINERVA/guardian-watchdog ili novi sektor?
- D-224-2: mouth 503 „loading“ ostaje „bad“ ili preuzimamo ocjenu nerve?
- D-224-3: cortex obnavlja incidente iz nerve `/state` pri startu — slot 237 to radi, treba samo potvrda.
- D-224-4: nerve dodaje komponente gaia-cortex i openmind-queue (djelomično slot 238).

**230 gaia-realtest-scenarios** — `swarm/gaia-realtest-scenarios` `dae33212` (LOKALNO)
- D-230-1: stog treba 54 porta, a raspon slota ima 10, treba veći raspon.
- D-230-2: cilj popravka mora biti u Gaia TEST_PORTS 19000–19999.
- D-230-3: CI nikad ne pokreće odobreni put restarta (samo plan), je li to u redu?
- D-230-4: hades-gate `check` vraća „WouldApply“ (dry-run), treba li pravi odgovor?

## HADES i sigurnost

**208 hades-safety-corpus** — `swarm/hades-safety-corpus` `3c305863`
- D-208-1: prihvatiti stroža pravila (ponekad preoprezna)?
- D-208-2: cloudflared odobriv ili zaštićen?
- D-208-3: rsync kao prijenos za backup?
- D-208-4: rsyslog, stari hermes i link-watch, zaštititi ili ne?
- D-208-5 / D-208-6: jedan izvor za popis „nikad ne skidati“ i odbijanje ciljeva zadanih adresom.

**209 hades-rustflags-guard** — `swarm/hades-rustflags-guard` `b1873983`
- D-209-1: strogo na svim ciljevima (napravljeno) ili samo x86_64?
- D-209-2 / D-209-3: dopustiti SIMD u dalek ili prijeći na Ed25519 bez SIMD-a?
- D-209-4: izlaz za nuždu (ne preporučuje se).
- D-209-5: edge/openmind-node ima istu rupu, popraviti?

**215 hades-key-custody** — `swarm/hades-key-custody` `84e988f` (LOKALNO)
- D-KC-1..11: uređaj za seed, zaštita i backup, putanje i prava, ritam rotacije, reload servera, otisak u ledgeru, opoziv i TTL. Nalaz: server zadrži stari ključ do restarta.

**204 galion-os-image-format** — `swarm/galion-os-image-format` `f35f7354` (LOKALNO)
- D-IF1: ključ slike = HADES admin ključ ili zaseban?
- D-IF2 / D-IF3 / D-IF4: softverski SHA ili hardverski, brojač protiv vraćanja verzije i gdje se slika gradi.

**201 sec-deny-wildcards** — `swarm/sec-deny-wildcards` `24829093`
- D-201-1: repo-check `publish` kao greška ili upozorenje?
- D-201-2: `publish = false` po crateu ili naslijeđeno iz workspacea?

**219 sec-ledger-race** — `swarm/sec-ledger-race` `8234150`
- D-219-1: oštećen kraj ledgera, ručni popravak ili automatski?
- D-219-2: ignorirati SIGXFSZ u hades-gate (treba `libc`)?
- D-219-3: testovi na tmpfs i read-only FS blokirani na boxu, pokrenuti na VPS-u?
- D-219-4: testovi pokreću `mkfifo` (GPL alat kao program), isto kao D-A6.

**220 sec-denylist-parity** — `swarm/sec-denylist-parity` `688d8835` (LOKALNO)
- D-220-1: zadržati zabranu `kexec_file_load` na armv7.
- D-220-2: zadržati nove zabrane `quotactl_fd` i `open_tree_attr`.
- D-220-3: zabraniti `modify_ldt` i na x86_64.

**225 sec-testnet-fixtures** — `swarm/sec-testnet-fixtures` `f977cc15` (LOKALNO)
- D-225-1: provjera 10 preskače CGNAT adrese?
- D-225-2: IP VPS-a u povijesti gita (veže se uz plan čišćenja povijesti).
- D-225-3: zastarjeli odlomak o Francuskoj u SOUL.md.

**229 sec-unsafe-safety** — `swarm/sec-unsafe-safety` `75a448f4`
- D-229-1: komentar SAFETY kao greška i clippy lint za sandbox crateove?
- D-229-2: prenijeti WNOWAIT popravak u edge sandbox.
- D-229-3: blaži oblik SAFETY komentara.

**234 sec-tooling-licence-memo** — `swarm/sec-tooling-licence-memo` `de856562` (LOKALNO)
- L1–L4, L6: GPL alati kao zasebni programi, UAPI činjenice i odbijanje BFG/filter-repo su DA. Treba samo tvoja potvrda.
- L5: CodeQL ne smijemo dok je repo privatan, a smijemo nakon javnog MIT izdanja.
- L7 / L8: dodati Apache-2.0 iznimku i odlučiti o 5 glibc binarnih datoteka u `dist/bin`.

## CI, build i dokumentacija

**196 ci-toolchain-pin** — `swarm/ci-toolchain-pin` `e871395a`
- D-196-1: `rust-version` 1.99 kao pin bez traženja MSRV-a.
- D-196-2: ciljevi po poslu ili u datoteci toolchaina.
- ✅ D-196-3: redoslijed spajanja s 38a26cb — agenti sad spajaju sami.

**202 ci-fuzz-workflow** — `swarm/ci-fuzz-workflow` `3a188b98`
- D-202-1: 60 s po cilju na svakom PR-u (~2–3 min).
- D-202-2: tjedni nalazi ne otvaraju issue automatski.
- D-202-3: imena varijabli `CORTEX_FUZZ_*` ili `GALION_FUZZ_*`.
- D-202-4: IPv6 provjera stroža od fail2ban standarda.

**211 int5-candle-repocheck** — `swarm/int5-candle-repocheck` `58a89bce`
- D-211-1: prihvatiti sporiji skalarni candle na aarch64 ili ga izuzeti?
- ✅ D-211-2: spoj u integration-5 — agenti sad spajaju sami.

**213 ci-realtest-with-nerve** — `swarm/ci-realtest-with-nerve` `7a95e068`
- D-213-1: raspon portova slota premalen za stog (treba 52).
- D-213-2: popravak apsolutne putanje ili `$GITHUB_WORKSPACE` u yaml-u.
- D-213-3: GitHub Actions još nije pokrenut (nema PR-a).

**216 / 228 ci-release-artefacts, ci-artefact-rehearsal** — `swarm/ci-release-artefacts` `04ce8743`, `swarm/ci-artefact-rehearsal` `4660283` (LOKALNO)
- D-228-1 / D-228-2: `--help` ne smije pokretati servis i treba vraćati 0 za svih 14 binarnih.
- D-228-3: ugraditi oznaku commita u sve binarne datoteke.
- ✅ D-228-4: ponovni deploy hermes-gateway iz izdanja — pokriveno odobrenjem živih prelazaka.

**197 whitepaper-factcheck** — `swarm/whitepaper-factcheck` `58d0d73e`
- DF-1: naslov o paralelnosti (prijedlog: „zaustavljanja prate navale startova“).
- DF-2: opseg rada, samo run od 20:34 ili cijeli swarm?
- ✅ DF-3: tekst „cortex deployan“ — cortex je od 21. 10. stvarno živ.

**231 docs-status-refresh** — `swarm/docs-status-refresh` `0d9c4bb7`
- D14: odobriti bilješke u `main.md`; REUSE.toml popravak za 7 datoteka još nije napravljen.

**243 console-v2-audit** — `swarm/console-v2-audit` `2a012311` (LOKALNO)
- R1: CSS budžet podijeliti na 20 + 8 KiB.
- R2: maknuti SPA_FIRST (hermes odmah, arena i bank nakon gašenja Vitea).
- R3: obrisati zastarjeli test `real_console_ui`.
- R4: miniz_oxide za gzip.
- ✅ R5: gašenje Vite dev servera na :8080 — pokriveno odobrenjem živih prelazaka, ide u zaseban slot.

## Hermes, memorija i spajanje

**198 hermes-mem-growth** — `swarm/hermes-mem-growth` `c2457cd4` (LOKALNO)
- ✅ D-198-1: spajanje popravka — agenti sad spajaju sami.
- D-198-2: JsonlStore drži indeks od ~480 B po sesiji i prepisuje `index.json` dvaput po razgovoru.
- D-198-3: brojači poziva po sesiji u hermes-tools se nikad ne brišu (~100 B po sesiji).
- D-198-4: sandbox korisnik na VPS-u, `nobody` ili trajni `sbxtest`?

**240 serde-pin-mergefix** — `swarm/serde-pin-mergefix` `41373013` (LOKALNO)
- D-SOP1 / D-SOP2: redoslijed ključeva u nerve `/health` promijenit će se pri sljedećem deployu, treba izgraditi nerve i shadow zajedno.

**242 shared-tar-mergefix** — `swarm/shared-tar-mergefix` `4eca4b7` (LOKALNO)
- D-242-1: gdje živi galion-tar (survival, deploy-switch ili core)?

Slotovi 238 i 239 nemaju novih odluka. Slot 244 se još radi.
