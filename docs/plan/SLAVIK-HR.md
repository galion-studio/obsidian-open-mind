# Galion OS — jedan plan (još bez gradnje)

Galion OS je glavno admin sučelje: **sigurno, privatno, anonimno**. Ne zamjenjuje Android — ARM sandbox radi **pored** tvojih aplikacija.

U planu su i: veza telefona (bežični adb preko VPS-a), Hermes Rust V2 (Revolut u opsegu, **bez** slanja novca), benchmarki (banka / Wall Street / automat — 100% na mock/paper).

Prije ičega: potvrdi točan model (Redmi Note 13 Pro / Note 9 Pro / drugi) iz Postavke → O telefonu.

Puni plan: [PLAN.md](./PLAN.md)

## Model prijetnji (2026-10-21 06:38 CEST, +02:00)
Puni dokument: [../security/threat-model.md](../security/threat-model.md). Najvažnije:
- **F7:** daljinsko upravljanje ekranom ne smije tipkati u bankovnim aplikacijama — most to blokira na samom telefonu, inače zabrana slanja novca ne vrijedi.
- **F2:** adb preko tunela = tko ima server, ima telefon; tunel ide na poseban korisnik bez roota, adb samo dok si prisutan.
- **F1:** proot nije sigurnosna granica; HADES ključ i most žive u odvojenoj aplikaciji od Linux sandboxa.
- **F6:** HADES ključ u hardverskom spremištu, neizvoziv, potpis uz PIN/otisak za rizične radnje.
- Odluke za tebe: D-OS1 … D-OS7 (§8 dokumenta).
