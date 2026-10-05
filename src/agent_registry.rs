//! The spawn registry (`.flywheel/agent-names.json`) as the assignee dialect.
//!
//! A flywheel project registers each herdr pane at spawn time: the pane NAME
//! (`flywheel-beads-oc`) and the mail IDENTITY pin it resolves to
//! (`GreenStream`). `br update --assignee` stores the PIN, because that is the
//! identity every other plane (mail, reservations, reclaim) agrees on; a
//! herdr name stored verbatim reads as a stranger to the orchestrator.
//!
//! The registry is deliberately optional: a workspace without one (plain
//! `br init` projects) keeps free-form assignees. When it exists, an explicit
//! herdr-name-shaped input must be registered — a typo is refused with the fix
//! instead of silently poisoning the claim.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::BeadsError;

/// One spawn-registry row. Every field is optional on the wire: older
/// flywheel versions wrote fewer keys, and a half-populated row must degrade,
/// not fail the read.
#[derive(Debug, Clone, Deserialize, Default)]
struct RegistryEntry {
    #[serde(default)]
    pane_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mail_identity: Option<String>,
}

/// The project's spawn registry, loaded from `.flywheel/agent-names.json`
/// next to the beads directory.
#[derive(Debug, Clone)]
pub struct SpawnRegistry {
    entries: Vec<RegistryEntry>,
}

/// What the assignee fold should store for an input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AssigneeFold {
    /// Store this pin (the input was already a pin, or resolved to one).
    Pin(String),
    /// Store the input as-is: it is not herdr-name-shaped (human names,
    /// emails, free-form teams).
    Verbatim(String),
    /// A herdr-name-shaped string this registry does not know.
    UnknownHerdrName(String),
}

impl SpawnRegistry {
    /// Load the registry for `beads_dir`, if the project has one. A missing
    /// file is normal (plain projects); a malformed one degrades to `None`
    /// so a corrupt registry never blocks ordinary assigns.
    pub fn load(beads_dir: &Path) -> Option<Self> {
        let path = registry_path(beads_dir)?;
        let contents = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<Vec<RegistryEntry>>(&contents) {
            Ok(entries) => Some(Self { entries }),
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "Ignoring malformed spawn registry; assignees pass through unchanged"
                );
                None
            }
        }
    }

    fn entry_by_name(&self, name: &str) -> Option<&RegistryEntry> {
        self.entries
            .iter()
            .find(|entry| entry.name.as_deref() == Some(name))
    }

    fn known_pin(&self, pin: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.mail_identity.as_deref() == Some(pin))
    }

    /// The one resolution rule: a registered herdr name folds to its pin, a
    /// known pin passes verbatim, an unregistered herdr-shaped name is
    /// flagged, and anything else passes verbatim.
    fn fold(&self, input: &str) -> AssigneeFold {
        if let Some(entry) = self.entry_by_name(input) {
            return match entry.mail_identity.as_deref() {
                Some(pin) => AssigneeFold::Pin(pin.to_string()),
                None => AssigneeFold::UnknownHerdrName(input.to_string()),
            };
        }
        if self.known_pin(input) || !is_herdr_name_shaped(input) {
            return AssigneeFold::Verbatim(input.to_string());
        }
        AssigneeFold::UnknownHerdrName(input.to_string())
    }

    /// Render how a stored assignee resolves, for `br show`: both directions
    /// (`pin -> pane` and legacy `name -> pin`) so an operator can see why a
    /// stored value might read wrong to the orchestrator.
    pub fn describe(&self, stored: &str) -> Option<String> {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.mail_identity.as_deref() == Some(stored))
        {
            let name = entry.name.as_deref().unwrap_or("unnamed pane");
            return Some(match entry.pane_id.as_deref() {
                Some(pane) => format!("herdr {name}, pane {pane}"),
                None => format!("herdr {name}"),
            });
        }
        self.entry_by_name(stored)
            .and_then(|entry| entry.mail_identity.as_deref())
            .map(|pin| format!("pin {pin}"))
    }
}

fn registry_path(beads_dir: &Path) -> Option<PathBuf> {
    Some(
        beads_dir
            .parent()?
            .join(".flywheel")
            .join("agent-names.json"),
    )
}

/// Fold an assignee input for `beads_dir` to the value the update stores.
///
/// `explicit` marks the `br update --assignee` flag (as opposed to a
/// claim-derived actor): only an explicit input is REFUSED when it is
/// herdr-name-shaped and unregistered. A claim-derived actor folds to its pin
/// when registered and otherwise passes through untouched, so agents whose
/// harness never registered a pane keep claiming exactly as before.
///
/// # Errors
///
/// Refuses an explicit herdr-name-shaped input the project has no
/// registration for, naming the fix (use the pin / register the pane) so a
/// claim never lands under an identity the orchestrator cannot resolve.
pub fn fold_assignee_for_store(
    beads_dir: &Path,
    input: &str,
    explicit: bool,
) -> Result<String, BeadsError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(input.to_string());
    }
    let Some(registry) = SpawnRegistry::load(beads_dir) else {
        return Ok(trimmed.to_string());
    };
    match registry.fold(trimmed) {
        AssigneeFold::Pin(pin) => Ok(pin),
        AssigneeFold::Verbatim(value) => Ok(value),
        AssigneeFold::UnknownHerdrName(name) if explicit => Err(BeadsError::validation(
            "assignee",
            format!(
                "'{name}' looks like a herdr pane name but this project's \
                 .flywheel/agent-names.json does not register it; claim with the pin \
                 (mail identity) instead, or register the pane first"
            ),
        )),
        AssigneeFold::UnknownHerdrName(name) => Ok(name),
    }
}

/// A herdr pane name: lowercase alphanumeric segments joined by single
/// hyphens (`flywheel-beads-oc`, `oc2-qa`). Pins (`GreenStream`) and free
/// strings (`alice`, `team@example.com`) never match.
fn is_herdr_name_shaped(value: &str) -> bool {
    let mut segments = value.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    let shaped_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    };
    if !shaped_segment(first) {
        return false;
    }
    let mut count = 1;
    for segment in segments {
        count += 1;
        if !shaped_segment(segment) {
            return false;
        }
    }
    count >= 2
}
