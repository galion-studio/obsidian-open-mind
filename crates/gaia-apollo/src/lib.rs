// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! APOLLO: the knowledge archive. In the story its whole archive was erased
//! by one man's override, so the reborn humans grew up without it. Here it
//! owns the memory gateway, link-watch intake, TEACHER and docs knowledge,
//! session history and the stack schema, held in a bounded in-memory archive.
//! APOLLO never purges: a purge request is refused and has to go through a
//! HADES plan with an Alpha's approval. No single key can erase it.

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Message, Outcome, Report,
    SubordinateFunction, System, Verb,
};

/// Archive entries kept at most; the oldest are evicted first.
pub const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub system: System,
    pub key: String,
    pub text: String,
    pub at: String,
}

#[derive(Debug, Default)]
pub struct Apollo {
    entries: Vec<Entry>,
    evicted: u64,
    rejected_links: u64,
}

impl Apollo {
    pub fn new() -> Apollo {
        Apollo::default()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn store(&mut self, system: System, key: &str, text: &str, at: &str) {
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.system == system && e.key == key)
        {
            e.text = text.to_string();
            e.at = at.to_string();
            return;
        }
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
            self.evicted += 1;
        }
        self.entries.push(Entry {
            system,
            key: key.to_string(),
            text: text.to_string(),
            at: at.to_string(),
        });
    }

    /// Case-insensitive search over keys and text.
    pub fn recall(&self, query: &str) -> Vec<&Entry> {
        let q = query.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.key.to_ascii_lowercase().contains(&q) || e.text.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    /// Accept only http(s) links with a host part.
    pub fn valid_link(s: &str) -> bool {
        let rest = s
            .strip_prefix("https://")
            .or_else(|| s.strip_prefix("http://"));
        matches!(rest, Some(r) if !r.is_empty() && !r.starts_with('/') && !r.contains(char::is_whitespace))
    }

    fn synthetic(&mut self, d: &Directive, at: &str) -> u64 {
        let n = scaled(8, d.intensity);
        for i in 0..n {
            self.store(
                d.target,
                &format!("{}-{i}", d.target),
                "synthetic entry",
                at,
            );
        }
        n
    }
}

impl SubordinateFunction for Apollo {
    fn id(&self) -> FunctionId {
        FunctionId::Apollo
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::MemoryGateway,
            System::LinkWatch,
            System::TeacherDocs,
            System::SessionHistory,
            System::StackSchema,
        ]
    }

    fn health(&self) -> Health {
        Health::ok((self.entries.len() * 100 / MAX_ENTRIES) as u8)
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        let at = ctx.at.clone();
        if d.payload.to_ascii_lowercase().contains("purge") || d.verb == Verb::Execute {
            return Report::for_directive(d, Outcome::Refused).summary(
                "APOLLO never purges; deletion is a HADES plan that needs an Alpha's approval",
            );
        }
        let report = match (d.target, d.verb) {
            (_, Verb::Status) => Report::for_directive(d, Outcome::Done)
                .summary(format!("{} entries", self.entries.len())),
            (System::MemoryGateway, _) => {
                if let Some(q) = d.payload.strip_prefix('?') {
                    let hits = self.recall(q.trim()).len();
                    Report::for_directive(d, Outcome::Done)
                        .summary(format!("{hits} hit(s) for {:?}", q.trim()))
                        .metric("hits", hits as i64)
                } else if let Some((k, v)) = d.payload.split_once('=') {
                    self.store(System::MemoryGateway, k.trim(), v.trim(), &at);
                    Report::for_directive(d, Outcome::Done)
                        .summary(format!("stored {:?}", k.trim()))
                } else {
                    let n = self.synthetic(d, &at);
                    Report::for_directive(d, Outcome::Done)
                        .summary(format!("archived {n} entries"))
                        .metric("stored", n as i64)
                }
            }
            (System::LinkWatch, _) => {
                let (mut ok, mut bad) = (0, 0);
                for l in d.payload.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    if Apollo::valid_link(l) {
                        self.store(System::LinkWatch, l, "link", &at);
                        ok += 1;
                    } else {
                        bad += 1;
                    }
                }
                if d.payload.is_empty() {
                    ok = self.synthetic(d, &at) as i64;
                }
                self.rejected_links += bad as u64;
                Report::for_directive(d, Outcome::Done)
                    .summary(format!("{ok} link(s) taken in, {bad} rejected"))
                    .metric("accepted", ok)
                    .metric("rejected", bad)
            }
            (System::TeacherDocs, _) => {
                let headings: Vec<&str> = d
                    .payload
                    .lines()
                    .filter_map(|l| l.strip_prefix('#'))
                    .map(|h| h.trim_start_matches('#').trim())
                    .filter(|h| !h.is_empty())
                    .collect();
                for h in &headings {
                    self.store(System::TeacherDocs, h, "lesson", &at);
                }
                let n = if headings.is_empty() {
                    self.synthetic(d, &at)
                } else {
                    headings.len() as u64
                };
                Report::for_directive(d, Outcome::Done)
                    .summary(format!("indexed {n} lesson heading(s)"))
                    .metric("indexed", n as i64)
            }
            (System::SessionHistory, _) => {
                let key = format!("session-{}", ctx.tick);
                let text = if d.payload.is_empty() {
                    "turn"
                } else {
                    d.payload.as_str()
                };
                self.store(System::SessionHistory, &key, text, &at);
                Report::for_directive(d, Outcome::Done).summary(format!("appended {key} at {at}"))
            }
            (System::StackSchema, _) => {
                let text = if d.payload.is_empty() {
                    "version=1\nprocessor=runs the stack\ncard=runs only the model"
                } else {
                    d.payload.as_str()
                };
                let fields: Vec<(&str, &str)> = text
                    .lines()
                    .filter_map(|l| l.split_once('='))
                    .map(|(k, v)| (k.trim(), v.trim()))
                    .collect();
                let has_version = fields.iter().any(|(k, _)| *k == "version");
                if !d.payload.is_empty() && !has_version {
                    Report::for_directive(d, Outcome::Refused)
                        .summary("schema needs a version field")
                } else {
                    for (k, v) in &fields {
                        self.store(System::StackSchema, k, v, &at);
                    }
                    Report::for_directive(d, Outcome::Done)
                        .summary(format!("schema with {} field(s) recorded", fields.len()))
                        .metric("fields", fields.len() as i64)
                }
            }
            (other, _) => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not an APOLLO system")),
        };
        ctx.env.knowledge = self.entries.len() as u32;
        report
    }

    fn on_message(&mut self, m: &Message, ctx: &mut Context) {
        // APOLLO keeps a record of every broadcast it hears.
        let key = format!("msg-{}", m.seq);
        let text = format!("{} from {}", m.topic, m.from.name());
        self.store(System::SessionHistory, &key, &text, &ctx.at.clone());
        ctx.env.knowledge = self.entries.len() as u32;
    }

    fn reset(&mut self) {
        // The archive survives a reboot; only counters reset.
        self.rejected_links = 0;
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("entries", self.entries.len())
            .with("cap", MAX_ENTRIES)
            .with("evicted", self.evicted)
            .with("rejected_links", self.rejected_links)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    fn run(a: &mut Apollo, d: Directive) -> Report {
        let mut env = Environment::pristine();
        let mut ctx = Context::new(3, "2026-10-18T21:03:00+02:00".into(), false, &mut env);
        a.handle(&d, &mut ctx)
    }

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Apollo::new().systems(), FunctionId::Apollo.systems());
    }

    #[test]
    fn store_and_recall() {
        let mut a = Apollo::new();
        run(
            &mut a,
            Directive::new(1, System::MemoryGateway, Verb::Run).payload("owner=Galion"),
        );
        let r = run(
            &mut a,
            Directive::new(2, System::MemoryGateway, Verb::Run).payload("?galion"),
        );
        assert_eq!(r.metric_value("hits"), Some(1));
    }

    #[test]
    fn purge_is_refused() {
        let mut a = Apollo::new();
        run(
            &mut a,
            Directive::new(1, System::MemoryGateway, Verb::Run).payload("k=v"),
        );
        let r = run(
            &mut a,
            Directive::new(2, System::MemoryGateway, Verb::Run).payload("purge all"),
        );
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(a.entries().len(), 1);
    }

    #[test]
    fn link_intake_validates() {
        let mut a = Apollo::new();
        let r = run(
            &mut a,
            Directive::new(1, System::LinkWatch, Verb::Run)
                .payload("https://example.org/a\nftp://x\nhttp://\n"),
        );
        assert_eq!(r.metric_value("accepted"), Some(1));
        assert_eq!(r.metric_value("rejected"), Some(2));
    }

    #[test]
    fn archive_is_bounded() {
        let mut a = Apollo::new();
        for i in 0..(MAX_ENTRIES + 10) {
            run(
                &mut a,
                Directive::new(1, System::MemoryGateway, Verb::Run).payload(format!("k{i}=v")),
            );
        }
        assert_eq!(a.entries().len(), MAX_ENTRIES);
        assert_eq!(a.evicted, 10);
    }

    #[test]
    fn schema_needs_version() {
        let mut a = Apollo::new();
        let r = run(
            &mut a,
            Directive::new(1, System::StackSchema, Verb::Run).payload("card=model"),
        );
        assert_eq!(r.outcome, Outcome::Refused);
        let r = run(
            &mut a,
            Directive::new(2, System::StackSchema, Verb::Run).payload("version=1\ncard=model"),
        );
        assert_eq!(r.metric_value("fields"), Some(2));
    }
}
