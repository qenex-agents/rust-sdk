//! Seat identity and the environment contract of the QENEX seat runtime.
//!
//! Implements R1 (detection) and the seat identity half of R4 from the shared
//! QENEX seat binding contract:
//!
//! * `QENEX_SEAT` - the seat identifier. Absent or empty means the process is
//!   not seat-bound.
//! * `QENEX_SEAT_WORKSPACE` - the absolute path of the seat working tree. It
//!   is the only writable root of a seat-bound runtime.
//! * `QENEX_SEAT_API` - optional base URL of the seat runtime service.
//! * `QENEX_SEAT_TOKEN` - bearer token for that service. It is never read,
//!   logged or written by this crate.
//! * `QENEX_SEAT_ALLOW_GLOBAL=1` - operator escape hatch that disables the
//!   write confinement.

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

/// Seat identifier. A runtime with this set is seat-bound.
pub const SEAT_ENV: &str = "QENEX_SEAT";
/// Absolute path of the seat working tree.
pub const WORKSPACE_ENV: &str = "QENEX_SEAT_WORKSPACE";
/// Optional base URL of the seat runtime service.
pub const API_ENV: &str = "QENEX_SEAT_API";
/// Bearer token for the seat runtime service. Never read by this crate.
pub const TOKEN_ENV: &str = "QENEX_SEAT_TOKEN";
/// Escape hatch that disables write confinement.
pub const ALLOW_GLOBAL_ENV: &str = "QENEX_SEAT_ALLOW_GLOBAL";

/// Why a seat binding could not be established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatError {
    /// `QENEX_SEAT` is unset or empty: the runtime is not seat-bound.
    NotSeatBound,
    /// `QENEX_SEAT` is set but `QENEX_SEAT_WORKSPACE` is missing.
    MissingWorkspace { seat: String },
}

impl fmt::Display for SeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeatError::NotSeatBound => write!(
                f,
                "{SEAT_ENV} is not set; the seat runtime only serves a single seat"
            ),
            SeatError::MissingWorkspace { seat } => write!(
                f,
                "{SEAT_ENV} is set to {seat:?} but {WORKSPACE_ENV} is not set; \
                 set {WORKSPACE_ENV} to the seat workspace, or unset {SEAT_ENV}"
            ),
        }
    }
}

impl std::error::Error for SeatError {}

/// An established seat binding: which seat, which workspace, and whether the
/// operator has lifted write confinement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatBinding {
    seat: String,
    workspace: PathBuf,
    api: Option<String>,
    allow_global: bool,
}

impl SeatBinding {
    /// Read the seat binding from the process environment.
    pub fn from_env() -> Result<Self, SeatError> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Build a binding from an arbitrary environment lookup.
    ///
    /// The lookup indirection keeps the detection logic testable without
    /// mutating the process environment (which is unsafe in edition 2024).
    pub fn from_lookup<F>(lookup: F) -> Result<Self, SeatError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let seat = lookup(SEAT_ENV).unwrap_or_default();
        if seat.trim().is_empty() {
            return Err(SeatError::NotSeatBound);
        }
        let workspace = lookup(WORKSPACE_ENV).unwrap_or_default();
        if workspace.trim().is_empty() {
            return Err(SeatError::MissingWorkspace { seat });
        }
        Ok(Self {
            seat,
            workspace: PathBuf::from(workspace),
            api: lookup(API_ENV).filter(|v| !v.trim().is_empty()),
            allow_global: lookup(ALLOW_GLOBAL_ENV)
                .map(|v| v.trim() == "1")
                .unwrap_or(false),
        })
    }

    /// The seat identifier, for example `coder`.
    pub fn seat(&self) -> &str {
        &self.seat
    }

    /// The seat working tree: the only writable root of a seat-bound runtime.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Optional base URL of the seat runtime service.
    pub fn api(&self) -> Option<&str> {
        self.api.as_deref()
    }

    /// Whether the operator lifted write confinement.
    pub fn allow_global(&self) -> bool {
        self.allow_global
    }

    /// The identity line required by R4.
    pub fn info_line(&self) -> String {
        format!("seat={} workspace={}", self.seat, self.workspace.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: Vec<(&'static str, &'static str)>) -> impl Fn(&str) -> Option<String> {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn unbound_without_seat_is_refused() {
        let err = SeatBinding::from_lookup(lookup(vec![(WORKSPACE_ENV, "/srv/seat")]))
            .expect_err("a runtime without QENEX_SEAT must not bind");
        assert_eq!(err, SeatError::NotSeatBound);
    }

    #[test]
    fn bound_seat_reports_identity() {
        let binding = SeatBinding::from_lookup(lookup(vec![
            (SEAT_ENV, "coder"),
            (WORKSPACE_ENV, "/var/lib/qenex/seats/coder/workspace"),
        ]))
        .expect("a complete environment binds");
        assert_eq!(binding.seat(), "coder");
        assert_eq!(
            binding.workspace(),
            Path::new("/var/lib/qenex/seats/coder/workspace")
        );
        assert_eq!(
            binding.info_line(),
            "seat=coder workspace=/var/lib/qenex/seats/coder/workspace"
        );
        assert!(!binding.allow_global());
    }

    #[test]
    fn seat_without_workspace_is_refused() {
        let err = SeatBinding::from_lookup(lookup(vec![(SEAT_ENV, "coder")]))
            .expect_err("a seat without a workspace must not bind");
        assert_eq!(
            err,
            SeatError::MissingWorkspace {
                seat: "coder".to_string()
            }
        );
    }

    #[test]
    fn allow_global_is_opt_in_only() {
        let binding = SeatBinding::from_lookup(lookup(vec![
            (SEAT_ENV, "cos"),
            (WORKSPACE_ENV, "/srv/cos"),
            (ALLOW_GLOBAL_ENV, "1"),
        ]))
        .expect("binding");
        assert!(binding.allow_global());
    }
}
