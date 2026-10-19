// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use galleon_arena::{load_seed, redacted_homes, Gladiator, Kind, LoadError, SEED_IDS};

fn homes_dir() -> PathBuf {
    if let Some(live) = redacted_homes() {
        return live;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/homes")
}

fn snapshot(homes: &Path) -> Vec<(u64, SystemTime)> {
    let mut stamps = Vec::new();
    for id in SEED_IDS {
        for name in ["SOUL.md", "TASK.md"] {
            let meta = fs::metadata(homes.join(id).join(name)).unwrap();
            stamps.push((meta.len(), meta.modified().unwrap()));
        }
    }
    stamps
}

#[test]
fn loads_four_seed_homes_without_writing() {
    let homes = homes_dir();
    assert!(
        homes.join("g2-0").join("SOUL.md").is_file(),
        "missing seed homes at {}",
        homes.display()
    );
    let before = snapshot(&homes);
    let loaded = load_seed(&homes).unwrap();
    let after = snapshot(&homes);
    assert_eq!(before, after, "loader wrote a seed file");
    assert_eq!(loaded.len(), 4);
    let ids: Vec<&str> = loaded.iter().map(|seat| seat.id.as_str()).collect();
    assert_eq!(ids, SEED_IDS);

    let g20 = &loaded[0];
    let g21 = &loaded[1];
    let g22 = &loaded[2];
    let g23 = &loaded[3];
    assert_eq!(g20.kind, Kind::Persona);
    assert!(g20.voice.contains("Plain English"));
    assert!(!g20.voice.contains("Steam."));
    assert!(g20.pinned_skill.is_none());
    assert!(g20.config_path.is_empty());
    assert!(g20.members.is_empty());
    assert!(g20.soul_path.ends_with("g2-0/SOUL.md"));
    assert!(g20.task_path.ends_with("g2-0/TASK.md"));
    assert!(g21.voice.starts_with("Steam."));
    assert!(g21.pinned_skill.is_none());
    assert_eq!(g22.pinned_skill.as_deref(), Some("self-eval"));
    assert!(g22.voice.contains("Plain English"));
    assert_eq!(g20.voice, g23.voice);
    assert_ne!(g20.id, g23.id);

    let soul_a = fs::read(homes.join("g2-0").join("SOUL.md")).unwrap();
    let soul_b = fs::read(homes.join("g2-3").join("SOUL.md")).unwrap();
    assert_eq!(soul_a, soul_b, "g2-0 and g2-3 souls diverged");
    for seat in &loaded {
        assert!(seat.task.contains("Sealed task."));
        assert!(seat.task.contains("Do not write"));
        assert!(seat.task.contains("Name one failure"));
        assert_eq!(seat.kind.as_str(), "persona");
    }
    let _typed: &[Gladiator] = &loaded;
}

#[test]
fn missing_directory_is_not_created() {
    let missing = std::env::temp_dir().join(format!(
        "galleon-arena-absent-{}-{}",
        std::process::id(),
        "b49"
    ));
    let _ = fs::remove_dir_all(&missing);
    assert!(!missing.exists());
    let err = load_seed(&missing).unwrap_err();
    assert!(matches!(err, LoadError::NotADirectory(_)));
    assert!(!missing.exists());
}

#[test]
fn team_record_does_not_read_a_home() {
    let team = Gladiator::team(
        "arena-compare",
        SEED_IDS.iter().map(|id| (*id).to_string()).collect(),
    )
    .unwrap();
    assert_eq!(team.kind, Kind::Team);
    assert_eq!(team.members.len(), 4);
    assert!(team.soul_path.is_empty());
    assert!(Gladiator::team("", vec!["g2-0".to_string()]).is_err());
    assert!(Gladiator::team("arena-compare", Vec::new()).is_err());
}
