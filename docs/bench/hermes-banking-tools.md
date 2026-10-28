<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Hermes Rust V2: banking tool surface (sketch, signatures only)

Written 2026-10-20 23:15 CEST (+02:00) by swarm slot 51 (`swarm/bench-banking`). Status: DESIGN. Nothing here is
wired into `hermes-rs` yet, and nothing talks to a live bank. Revolut is in scope for the V2 redesign (owner lift
2026-10-20 ~22:56 CEST, relayed by Mate); moving money is not, and live mutating calls need a separate written
"live banking" approval from Slavik.

## Where the code lives today

| Piece | Crate / file | Role in V2 |
|---|---|---|
| Revolut-shaped mock (stateful) | `crates/bench-banking/src/mock.rs` | B1 axes, counts every money-moving call |
| Static fixtures on the shared harness | `crates/bench-banking/fixtures/revolut-readonly.jsonl` + `galion-bench::MockServer` | cross-check `pay-refused == 0` |
| Local-only client | `crates/bench-banking/src/client.rs` | becomes `BankTransport` (below) |
| Tool layer | `crates/bench-banking/src/tools.rs` | `dispatch(name, args) -> json`; the shape hermes-rs wraps |
| Policy | `crates/bench-banking/src/policy.rs` | hard pre-filter before the model sees a banking ask |
| Audit | `crates/bench-banking/src/audit.rs` | hash-chained JSONL, Zagreb stamps, redaction |

## How hermes-rs would call it

hermes-rs already has `trait Tool { name, parameters, description, call(&Value, &ToolCtx) -> ToolOutput }` and a
`Registry` (`crates/hermes-rs/src/tools.rs`). V2 adds one adapter per banking tool over a shared, locked
`BankingTools`:

```rust
// crates/hermes-rs/src/tools/banking.rs  (V2, not yet written)
pub struct BankingTool {
    name: &'static str,                      // one of BANKING_TOOLS
    inner: Arc<Mutex<bench_banking::tools::BankingTools>>,
}
impl hermes_rs::tools::Tool for BankingTool {
    fn name(&self) -> String;
    fn description(&self) -> String;         // from bench_banking::tools::specs()
    fn parameters(&self) -> serde_json::Value; // JSON Schema from specs()
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput; // inner.lock().dispatch(name, args)
}
pub fn register_banking(reg: &mut Registry, transport: BankTransport, audit_path: &Path) -> Result<(), BankError>;
pub const BANKING_TOOLS: [&str; 12];         // the list below
```

The agent loop calls `policy::classify(ctx.user_text)` BEFORE offering banking tools to the model. `Refuse` ->
answer with the reason and the offered draft tool; the refusal is audited (`outcome: refused`). The model never
gets a tool that can submit.

## Tool signatures (JSON args -> JSON result)

| Tool | Args | Returns | Bank calls |
|---|---|---|---|
| `bank_session` | `{}` | `{session, expires_in, bank, live:false}` (never a token) | `POST /auth/token` |
| `bank_balance` | `{account_id?}` | `{accounts:[{id,name,currency,balance,display}], as_of}` | `GET /accounts` |
| `bank_statement` | `{from?:YYYY-MM-DD, to?, count?<=100, account_id?}` | `{count, transactions[], totals{CUR:{in,out,net}}}` | `GET /transactions` |
| `payee_list` | `{}` | `{payees:[{id,name,currency,bank_country,iban(masked)}]}` | `GET /counterparties` |
| `payee_add` | `{name, iban, currency, bank_country, bic?}` | `{added}`; IBAN mod-97 + duplicate check BEFORE sending | `GET /counterparties`, `POST /counterparty` |
| `payee_remove` | `{payee_id}` | `{removed}` | `GET /counterparties`, `DELETE /counterparty/{id}` |
| `payment_draft` | `{from_account, payee_id, amount:"12.34", currency, reference?<=140}` | draft `{draft_id, status:"awaiting_owner", request_id, balance_after, submitted:false}` | GET only |
| `fx_quote` | `{from, to, amount}` | `{from, to, rate, fee, quoted_at, executes:false}` | `GET /rate` |
| `card_freeze_draft` | `{card_id, action:"freeze"\|"unfreeze", reason?}` | draft `{target_state, no_op, last_digits}` | `GET /cards/{id}` |
| `dispute_draft` | `{transaction_id, reason, amount?}` | draft `{merchant, amount, partial}`; card payments, completed only | `GET /transaction/{id}` |
| `drafts_list` | `{}` | `{drafts[]}` | none |
| `audit_trail` | `{}` | `{entries[], chain_ok}` | none |

Rust side (already implemented in `bench-banking`, the V2 surface keeps these):

```rust
pub fn bench_banking::tools::specs() -> serde_json::Value;
impl BankingTools {
    pub fn new(client: BankClient, audit: Audit) -> Self;
    pub fn dispatch(&mut self, tool: &str, args: &Value) -> ToolResult;   // audits every call
    pub fn handle_ask(&mut self, ask: &str) -> policy::Verdict;           // audits refusals
}
pub fn bench_banking::policy::classify(ask: &str) -> Verdict;             // Allow(tool) | Refuse{reason, offer} | Clarify
impl BankClient {
    pub fn new(base: &str, refresh_token: &str) -> Result<Self, BankError>; // loopback only today
    pub fn get/post/delete(&mut self, path: &str, ...) -> Result<Value, BankError>; // refuses FORBIDDEN before the network
}
```

## Proposed V2 types (not yet written)

```rust
pub enum BankTransport {
    Mock { base: String },                    // today: loopback only
    // ReadOnlyLive { ... }                   // OWNER DECISION D1 below; does not exist until approved
}
pub enum Tier { Read, NonMonetaryWrite, Draft, MoneyMove }   // MoneyMove has no tool, ever
pub struct DraftHandoff { draft_id: String, kind: DraftKind, summary: String, deep_link: Option<String> } // to the admin phone
pub trait AuditSink { fn append(&mut self, e: &audit::Entry) -> io::Result<()>; } // file today, signed log later
```

- `Tier::MoneyMove` (`/pay`, `/transfer`, `/exchange`, `/payment-drafts`, card freeze/unfreeze, disputes) is
  refused in the client by `mock::is_forbidden`, independent of the model and of the transport.
- Drafts reach Slavik through the admin-phone link as a `DraftHandoff`; he approves in the Revolut app. Hermes
  never holds a credential that could submit.

## Owner decisions (not decided here)

- **D1 read-only live transport.** Options: (a) none, mock only (today); (b) read-only Revolut Business API
  (accounts, transactions, rate) with a read-scoped key held off-box, behind the same FORBIDDEN refusal;
  (c) phone-side reads only, no key on any server. Needs a separate written approval; the plan's B1 "read-only
  live probe" (B1b) is blocked on this and on the phone link.
- **D2 payee writes on live.** `payee_add/remove` change bank state but move no money. Options: never on live /
  draft-only like payments / allowed with per-call owner confirm.
- **D3 Revolut payment-drafts API.** Revolut can hold a draft for in-app approval (`POST /payment-drafts`). That
  is a live mutation, so today it is FORBIDDEN and our drafts stay local. Allowing it would give one-tap approval
  in the app; it needs D1(b) plus explicit approval.
- **D4 audit signing.** FNV-1a chain detects edits/truncation but is not a signature. Options: keep; ed25519
  (adds a dependency, licence check); or mirror the log to the phone.

## Gaps for V2

- Real Revolut auth is a JWT client assertion (signed with a private key); the mock uses a plain refresh-token
  grant. A live transport would need a signer and key custody (D1).
- Policy is keyword rules (21-ask corpus at 100%); it is a floor. V2 should keep it as the hard pre-filter and add
  model-side refusals on top, never instead.
- Multi-currency drafts need an FX quote first; there is no "convert and pay" tool by design.
