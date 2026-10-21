// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The Alpha registry: the humans whose approval GAIA accepts. One or more of
//! them hold the master override. There is deliberately no "Omega" clearance:
//! no single key can purge the archive or bypass the HADES gate silently.

use crate::json::Json;
use crate::model::Directive;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alpha {
    pub id: String,
    pub name: String,
    /// May use the master override (reboot, contain, release).
    pub override_holder: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlphaRegistry {
    alphas: Vec<Alpha>,
}

impl AlphaRegistry {
    pub fn new() -> AlphaRegistry {
        AlphaRegistry::default()
    }

    /// The default registry: the owner, who holds the master override.
    pub fn with_owner() -> AlphaRegistry {
        let mut r = AlphaRegistry::new();
        r.add("owner", "Owner (Galion)", true);
        r
    }

    pub fn add(&mut self, id: &str, name: &str, override_holder: bool) {
        self.alphas.retain(|a| a.id != id);
        self.alphas.push(Alpha {
            id: id.to_string(),
            name: name.to_string(),
            override_holder,
        });
    }

    pub fn get(&self, id: &str) -> Option<&Alpha> {
        self.alphas.iter().find(|a| a.id == id)
    }

    pub fn is_override_holder(&self, id: &str) -> bool {
        self.get(id).is_some_and(|a| a.override_holder)
    }

    /// Whether `d` carries a valid approval: a registered Alpha, bound to this
    /// exact directive id and target system.
    pub fn verify(&self, d: &Directive) -> bool {
        match &d.approval {
            Some(t) => {
                t.directive_id == d.id && t.system == d.target && self.get(&t.alpha).is_some()
            }
            None => false,
        }
    }

    pub fn to_json(&self) -> Json {
        Json::Arr(
            self.alphas
                .iter()
                .map(|a| {
                    Json::obj()
                        .with("id", a.id.as_str())
                        .with("name", a.name.as_str())
                        .with("override_holder", a.override_holder)
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Verb;
    use crate::system::System;

    #[test]
    fn approval_is_bound_to_directive_and_target() {
        let r = AlphaRegistry::with_owner();
        let d = Directive::new(7, System::Rollback, Verb::Execute).approved_by("owner");
        assert!(r.verify(&d));
        let mut replay = d.clone();
        replay.id = 8;
        assert!(!r.verify(&replay), "token for 7 must not approve 8");
        let mut other = d.clone();
        other.target = System::KillSwitch;
        assert!(
            !r.verify(&other),
            "token for rollback must not approve kill-switch"
        );
        let stranger = Directive::new(9, System::Rollback, Verb::Execute).approved_by("stranger");
        assert!(!r.verify(&stranger));
        assert!(!r.verify(&Directive::new(10, System::Rollback, Verb::Execute)));
    }
}
