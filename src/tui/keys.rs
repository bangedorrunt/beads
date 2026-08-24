//! Keybinding registry (bead gu7ts.8). governed-by: ADR-0003.
//!
//! Mirrors bv's `keybindings.go` `GetKeyBindingDocs` — authoritative
//! source for the shortcuts sidebar and help overlay (bv UX map §3.14,
//! §5.1-5.2). Registration order is display order.

/// One keybinding doc entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    /// Key chord (e.g. "j", "ctrl+c", "?").
    pub key: &'static str,
    /// Human description.
    pub desc: &'static str,
    /// Category grouping (Navigation / Views / Filters / Actions / ...).
    pub category: &'static str,
    /// Contexts where the binding is active (e.g. "all", "list", "help").
    pub contexts: &'static [&'static str],
}

/// Full registry in display order (bv UX map §3.14).
#[must_use]
pub const fn registry() -> &'static [KeyBinding] {
    &REGISTRY
}

#[must_use]
pub fn all_bindings_for_focus(focus: &str) -> Vec<&'static KeyBinding> {
    REGISTRY
        .iter()
        .filter(|b| b.contexts.contains(&"all") || b.contexts.contains(&focus))
        .collect()
}

const REGISTRY: [KeyBinding; 26] = [
    KeyBinding {
        key: "j",
        desc: "Move down",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "k",
        desc: "Move up",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "G",
        desc: "Go to end",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "gg",
        desc: "Go to start",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "ctrl+d",
        desc: "Page down",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "ctrl+u",
        desc: "Page up",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "enter",
        desc: "Open details",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "esc",
        desc: "Back/close",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "q",
        desc: "Quit",
        category: "Navigation",
        contexts: &["all"],
    },
    KeyBinding {
        key: "b",
        desc: "Board view",
        category: "Views",
        contexts: &["list", "detail"],
    },
    KeyBinding {
        key: "g",
        desc: "Graph view",
        category: "Views",
        contexts: &["list", "detail"],
    },
    KeyBinding {
        key: "i",
        desc: "Insights panel",
        category: "Views",
        contexts: &["list", "detail"],
    },
    KeyBinding {
        key: "?",
        desc: "Help overlay",
        category: "Views",
        contexts: &["all"],
    },
    KeyBinding {
        key: ";",
        desc: "Shortcuts sidebar",
        category: "Views",
        contexts: &["all"],
    },
    KeyBinding {
        key: "p",
        desc: "Priority hints",
        category: "Views",
        contexts: &["list", "detail"],
    },
    KeyBinding {
        key: "/",
        desc: "Search/filter",
        category: "Filters",
        contexts: &["list"],
    },
    KeyBinding {
        key: "o",
        desc: "Open issues only",
        category: "Filters",
        contexts: &["list"],
    },
    KeyBinding {
        key: "c",
        desc: "Closed issues only",
        category: "Filters",
        contexts: &["list"],
    },
    KeyBinding {
        key: "r",
        desc: "Ready (unblocked)",
        category: "Filters",
        contexts: &["list"],
    },
    KeyBinding {
        key: "l",
        desc: "Label picker",
        category: "Filters",
        contexts: &["list"],
    },
    KeyBinding {
        key: "x",
        desc: "Export to markdown",
        category: "Actions",
        contexts: &["list", "detail"],
    },
    KeyBinding {
        key: "y",
        desc: "Copy issue ID",
        category: "Actions",
        contexts: &["all"],
    },
    KeyBinding {
        key: "C",
        desc: "Copy full issue",
        category: "Actions",
        contexts: &["detail"],
    },
    KeyBinding {
        key: "O",
        desc: "Open in $EDITOR",
        category: "Actions",
        contexts: &["detail"],
    },
    KeyBinding {
        key: "ctrl+r",
        desc: "Force refresh",
        category: "Actions",
        contexts: &["all"],
    },
    KeyBinding {
        key: "ctrl+j",
        desc: "Scroll sidebar down",
        category: "Navigation",
        contexts: &["all"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_not_empty() {
        assert!(!registry().is_empty());
    }

    #[test]
    fn all_bindings_for_list_includes_global_and_list() {
        let list = all_bindings_for_focus("list");
        assert!(list.iter().any(|b| b.key == "j"));
        assert!(list.iter().any(|b| b.key == "/"));
    }

    #[test]
    fn help_overlay_has_global_bindings() {
        let help = all_bindings_for_focus("help");
        // help should still see global nav keys
        assert!(help.iter().any(|b| b.key == "q"));
    }
}
