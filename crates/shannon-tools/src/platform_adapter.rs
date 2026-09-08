//! Desktop-control backend abstraction (T13 Tier 2 foundation).
//!
//! Today every platform runs the same screenshot + synthetic-input loop
//! (xcap + enigo). The macOS AX research (docs/plans/
//! 2026-09-06-p3-future-research.md §T13) shows an Accessibility-tree
//! backend is strictly better where available — 78–96% fewer tokens,
//! deterministic element refs, no cursor warping — so desktop control is
//! re-expressed over a [`PlatformAdapter`] seam:
//!
//! - [`MacosEnigoAdapter`] — the current path (CGEvent), kept as the
//!   macOS fallback for Electron/non-AX-aware apps.
//! - [`MacosAxAdapter`] — skeleton for the AXUIElement backend; lands
//!   with the Tier 2 implementation (via objc2, mirroring lahfir/
//!   agent-desktop). Reports itself unavailable until then.
//!
//! Linux/Windows adapters (X11, at-spi, UIA) slot into the same trait.

/// Element reference returned by AX-tree snapshots (e.g. "@e12"), stable
/// across small layout shifts — the property screenshots cannot offer.
pub type ElementRef = String;

/// What a backend can do; AX backends advertise structured actions, pixel
/// backends only raw input synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterCapabilities {
    /// Structured snapshot (roles/names/refs) instead of pixels.
    pub structured_snapshot: bool,
    /// Act on an [`ElementRef`] (press/set-value) without coordinates.
    pub element_actions: bool,
    /// Raw pixel screenshot + coordinate input (always true today).
    pub pixel_input: bool,
}

/// One desktop-control backend.
pub trait PlatformAdapter: Send + Sync {
    /// Stable backend name for diagnostics (`"macos-ax"`, `"macos-enigo"`).
    fn name(&self) -> &'static str;

    /// Whether the backend is usable in this session (permissions held,
    /// OS APIs present). Cheap — no side effects.
    fn available(&self) -> bool;

    fn capabilities(&self) -> AdapterCapabilities;
}

/// Current enigo/CGEvent path — the macOS baseline and universal fallback.
pub struct MacosEnigoAdapter;

impl PlatformAdapter for MacosEnigoAdapter {
    fn name(&self) -> &'static str {
        "macos-enigo"
    }

    fn available(&self) -> bool {
        // enigo is compiled in whenever desktop automation exists; the
        // Accessibility TCC grant is the runtime question, which surfaces
        // as action failures today.
        cfg!(feature = "computer-use")
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            structured_snapshot: false,
            element_actions: false,
            pixel_input: true,
        }
    }
}

/// AXUIElement backend — Tier 2 skeleton. `available()` reports `false`
/// until the objc2-based implementation lands; selection logic already
/// prefers it, so Tier 2 becomes a drop-in.
pub struct MacosAxAdapter;

impl PlatformAdapter for MacosAxAdapter {
    fn name(&self) -> &'static str {
        "macos-ax"
    }

    fn available(&self) -> bool {
        // Tier 2 implementation pending (see module docs). When it lands:
        // `AXIsProcessTrustedWithOptions` gates this.
        false
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            structured_snapshot: true,
            element_actions: true,
            pixel_input: false,
        }
    }
}

/// Pick the best available backend. macOS prefers AX when its TCC grant
/// is held and falls back to enigo; other platforms have exactly one
/// adapter today (enigo), so the macOS arms simply never run there.
pub fn select_desktop_adapter() -> Box<dyn PlatformAdapter> {
    #[cfg(target_os = "macos")]
    {
        let ax = MacosAxAdapter;
        if ax.available() {
            return Box::new(ax);
        }
        return Box::new(MacosEnigoAdapter);
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(MacosEnigoAdapter)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_ax_adapter_advertises_structured_capabilities() {
        let caps = MacosAxAdapter.capabilities();
        assert!(caps.structured_snapshot);
        assert!(caps.element_actions);
        assert!(!caps.pixel_input);
    }

    #[test]
    fn test_enigo_adapter_is_pixel_only() {
        let caps = MacosEnigoAdapter.capabilities();
        assert!(!caps.structured_snapshot);
        assert!(caps.pixel_input);
    }

    #[test]
    fn test_ax_skeleton_reports_unavailable() {
        // Tier 2 pending: selection must not hand out the AX backend while
        // it cannot act.
        assert!(!MacosAxAdapter.available());
    }

    #[test]
    fn test_selection_returns_a_named_backend() {
        let adapter = select_desktop_adapter();
        assert!(
            adapter.name() == "macos-ax" || adapter.name() == "macos-enigo",
            "unexpected adapter {}",
            adapter.name()
        );
        // Usability tracks the compiled feature: with `computer-use` the
        // enigo path is real; without it, the selected backend honestly
        // reports unavailable (tool execution returns the stub error).
        assert_eq!(adapter.available(), cfg!(feature = "computer-use"));
    }
}
