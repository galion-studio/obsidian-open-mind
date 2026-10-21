# Slavik: otvorene odluke (stanje 2026-10-21 06:40 CEST, +02:00)

Sve je pripremljeno i testirano; ovo čeka samo tvoje „da/ne”. Main je i dalje `13a296c`, ništa nije spojeno.

## 1. Spajanje u main
Spremno je `swarm/integration-4-squash` (`9b5f6ad`, 1262 testa prolaze, bez IP-a VPS-a), ali `cargo deny` još pada na zip (popravak candle-a nije uključen). Odluči spajamo li sada ili nakon popravka zipa, i daj način za push na main (alat smije samo `swarm/*`).

## 2. Prebacivanje tunera i nervea (D1–D4)
D1 (Rust galion-deploy) i tuner su već na VPS-u od 04:32, a nerve 8670 i dalje radi staru verziju u načinu „live” (novi nerve radi samo kao sjena na 8672). Odluči D2 (nerve u „dry-run” 24 h), D3 (smije li sigurnosna kopija sadržavati tajne) i D4 (termin, ne 25.10. od 01 do 04 h ni oko 02:20).

## 3. HADES: A, B ili C
A = samo bilježi, B = potpisani tipizirani koraci (preporuka, u prototipu `0f41f29` nije prošao nijedan od 164 napada), C = ostaje kako jest. Uz to treba tvoj HADES admin ključ, napravljen na tvom uređaju (na server ide samo javni dio).

## 4. Hermes s pravim modelom (H1)
Smije li jedan stvarni benchmark ići preko plaćenog modela: koji model i koliki limit u USD? Dok ne kažeš „da”, plaćeni model ostaje isključen.

## 5. Bankarstvo (4 odluke)
Odobri ili odbij: (1) čitanje pravog Revoluta samo za čitanje i gdje stoji ključ, (2) izmjene primatelja, (3) Revolut API za nacrte plaćanja, (4) potpisani zapis (audit) svake radnje. Pomicanje novca ostaje zabranjeno bez posebnog pisanog odobrenja.

## 6. Izvor podataka za burzovni test
Preporuka: sada sintetički podaci, kasnije javni isječak. Slobodni za spremanje su US Treasury, Fed H.10, ECB i NY Fed SOFR; Yahoo, FRED, Binance i slični nisu.

## 7. Senzori (cortex)
Novi senzori (ssh, ranjivosti, drift, veze, procesi, DNS/TLS, HADES) gotovi su, ali isključeni, a cortex još nije pokrenut na VPS-u. Odluči uključujemo li ih u produkciji i s kojim pragovima (preporuka: ddos težine 15/10/4).

## 8. Candle (popravak zip ranjivosti)
C1: zadržati našu zakrpanu kopiju candle-a (preporuka). C2: smijemo li javno zamoliti autore candle-a da prijeđu na zip 9?

## 9. Galion OS
Pošalji točan model mobitela (Postavke → O telefonu) i potvrdi da su sigurnost, privatnost i anonimnost obvezni za v1. Bez ta dva odgovora gradnja ne počinje.

## 10. Strože obrađivanje zahtjeva
Odobri stavke H1–H3, M1–M3, D1–D4, R1–R4 i C1–C5 u `docs/approvals/common-migrate-2-strict.md` na `swarm/common-migrate-2` (`123eab2`). Svaki servis može se kasnije vratiti zasebno.

## ✅ Gotovo
OpenRouter: dodano 50 USD kredita.
