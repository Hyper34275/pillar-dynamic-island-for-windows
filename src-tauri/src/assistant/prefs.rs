//! The saved mailbox preference ("remember my choice"): `state/assistant_prefs.json`.
//!
//! Only hashed mailbox ids are stored (never names, never query text). The preference also records
//! which searchable mailboxes existed when it was saved, so it is applied only while no mailbox has
//! appeared since: a new shared mailbox always triggers the question again.

use serde::{Deserialize, Serialize};

const FILE: &str = "assistant_prefs.json";
const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    pub version: u32,
    /// Mailbox ids the user chose to search.
    pub chosen: Vec<String>,
    /// Every searchable mailbox id that existed when the choice was saved.
    pub known: Vec<String>,
}

impl Prefs {
    pub fn new(chosen: Vec<String>, known: Vec<String>) -> Self {
        Prefs { version: VERSION, chosen, known }
    }

    /// The chosen mailboxes that are searchable now, when the preference may be applied: the
    /// current searchable set has no id that was not known at save time.
    pub fn applies(&self, searchable: &[String]) -> Option<Vec<String>> {
        if searchable.iter().any(|id| !self.known.contains(id)) {
            return None;
        }
        let ids: Vec<String> = searchable.iter().filter(|id| self.chosen.contains(id)).cloned().collect();
        (!ids.is_empty()).then_some(ids)
    }
}

pub fn parse(bytes: &[u8]) -> Option<Prefs> {
    let prefs: Prefs = serde_json::from_slice(bytes).ok()?;
    (prefs.version == VERSION).then_some(prefs)
}

pub fn to_bytes(prefs: &Prefs) -> Vec<u8> {
    serde_json::to_vec_pretty(prefs).unwrap_or_default()
}

fn path() -> Result<std::path::PathBuf, String> {
    Ok(crate::paths::state_dir()?.join(FILE))
}

/// The saved preference; a missing or unreadable file is "no preference".
pub fn load() -> Option<Prefs> {
    let bytes = std::fs::read(path().ok()?).ok()?;
    parse(&bytes)
}

pub fn save(prefs: &Prefs) -> Result<(), String> {
    let path = path()?;
    crate::reminder_state::write_atomic(&path, &to_bytes(prefs)).map_err(|e| format!("APP-043: preference not saved: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn applies_while_the_set_is_unchanged() {
        let p = Prefs::new(ids(&["a"]), ids(&["a", "b"]));
        assert_eq!(p.applies(&ids(&["a", "b"])), Some(ids(&["a"])));
        // a mailbox that went away does not matter
        assert_eq!(p.applies(&ids(&["a"])), Some(ids(&["a"])));
    }

    #[test]
    fn a_new_mailbox_cancels_the_preference() {
        let p = Prefs::new(ids(&["a"]), ids(&["a", "b"]));
        assert_eq!(p.applies(&ids(&["a", "b", "c"])), None);
    }

    #[test]
    fn nothing_chosen_is_searchable_means_ask() {
        let p = Prefs::new(ids(&["a"]), ids(&["a", "b"]));
        assert_eq!(p.applies(&ids(&["b"])), None);
    }

    #[test]
    fn round_trips_and_rejects_garbage() {
        let p = Prefs::new(ids(&["a"]), ids(&["a", "b"]));
        assert_eq!(parse(&to_bytes(&p)), Some(p));
        assert_eq!(parse(b"not json"), None);
        assert_eq!(parse(br#"{"version":9,"chosen":[],"known":[]}"#), None);
    }
}
