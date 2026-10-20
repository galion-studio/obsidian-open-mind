// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option (b) spike: an l1_records-like table in redb and an l1_fts-like
//! BM25 index in tantivy. It checks the operations openmind-memory needs:
//! upsert by key, read back, ranked full-text search, delete plus
//! re-index, and reopen after close. Prints PASS/FAIL lines and exits with
//! the number of failures.

use redb::{Database, ReadableTableMetadata, TableDefinition};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, TantivyDocument, Term};

const L1: TableDefinition<&str, &str> = TableDefinition::new("l1_records");

fn check(fails: &mut i32, what: &str, ok: Result<bool, String>) {
    match ok {
        Ok(true) => println!("PASS {what}"),
        Ok(false) => {
            *fails += 1;
            println!("FAIL {what}: wrong result");
        }
        Err(e) => {
            *fails += 1;
            println!("FAIL {what}: {e}");
        }
    }
}

fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut fails = 0;
    let rows = [
        ("r1", "Slavik prefers Rust for every service"),
        ("r2", "The VPS runs hermes and openmind memory"),
        ("r3", "Zagreb time is CEST plus two hours"),
    ];

    let db_path = dir.join("rows.redb");
    let r = (|| {
        let db = Database::create(&db_path).map_err(e)?;
        let tx = db.begin_write().map_err(e)?;
        {
            let mut t = tx.open_table(L1).map_err(e)?;
            for (k, v) in rows {
                t.insert(k, v).map_err(e)?;
            }
            // upsert: same key, new content (ON CONFLICT DO UPDATE)
            t.insert("r1", "Slavik prefers Rust for everything, no exceptions")
                .map_err(e)?;
        }
        tx.commit().map_err(e)?;
        let rx = db.begin_read().map_err(e)?;
        let t = rx.open_table(L1).map_err(e)?;
        let v = t.get("r1").map_err(e)?.map(|g| g.value().to_string());
        Ok(t.len().map_err(e)? == 3
            && v.as_deref() == Some("Slavik prefers Rust for everything, no exceptions"))
    })();
    check(
        &mut fails,
        "redb upsert + read back (3 rows, r1 updated)",
        r,
    );

    let r = (|| {
        let db = Database::create(&db_path).map_err(e)?;
        let rx = db.begin_read().map_err(e)?;
        let t = rx.open_table(L1).map_err(e)?;
        Ok(t.len().map_err(e)? == 3)
    })();
    check(&mut fails, "redb reopen after close", r);

    let idx_dir = dir.join("fts");
    let _ = std::fs::create_dir_all(&idx_dir);
    let mut sb = Schema::builder();
    let id = sb.add_text_field("record_id", STRING | STORED);
    let content = sb.add_text_field("content", TEXT);
    let schema = sb.build();
    let r = (|| {
        let index = Index::create_in_dir(&idx_dir, schema.clone()).map_err(e)?;
        let mut w: IndexWriter = index.writer(15_000_000).map_err(e)?;
        for (k, v) in rows {
            w.add_document(doc!(id => k, content => v)).map_err(e)?;
        }
        w.commit().map_err(e)?;
        // FTS rebuild of one row: delete by key, re-add (memory-schema rebuild)
        w.delete_term(Term::from_field_text(id, "r1"));
        w.add_document(
            doc!(id => "r1", content => "Slavik prefers Rust for everything, no exceptions"),
        )
        .map_err(e)?;
        w.commit().map_err(e)?;
        Ok(true)
    })();
    check(&mut fails, "tantivy index + delete/re-add + commit", r);

    let r = (|| {
        let index = Index::open_in_dir(&idx_dir).map_err(e)?;
        let reader = index.reader().map_err(e)?;
        let s = reader.searcher();
        let q = QueryParser::for_index(&index, vec![content])
            .parse_query("rust exceptions")
            .map_err(e)?;
        let top = s
            .search(&q, &TopDocs::with_limit(5).order_by_score())
            .map_err(e)?;
        let mut hits = Vec::new();
        for (score, addr) in top {
            let d: TantivyDocument = s.doc(addr).map_err(e)?;
            let k = d
                .get_first(id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            println!("  hit {k} bm25={score:.3}");
            hits.push((k, score));
        }
        Ok(hits.len() == 1 && hits[0].0 == "r1" && hits[0].1 > 0.0 && s.num_docs() == 3)
    })();
    check(
        &mut fails,
        "tantivy BM25 search after reopen (1 hit, r1, score > 0, 3 docs)",
        r,
    );

    println!("failures: {fails}");
    std::process::exit(fails);
}
