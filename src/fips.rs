// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Red Hat, Inc.

//! The crypto provider, and FIPS mode.
//!
//! Every cryptographic primitive this process runs goes through the system
//! OpenSSL. The ExtProc listener speaks TLS through OpenSSL directly
//! (`tokio-openssl`), and everything the Praxis filters do over TLS, such as
//! subrequests, goes through rustls, which performs no cryptography of its own
//! and delegates every primitive to a [`CryptoProvider`]. This process installs
//! exactly one, the OpenSSL-backed provider, before any filter or connector is
//! built. On a FIPS-enabled Red Hat Enterprise Linux host the library behind
//! both paths is the platform's validated module.
//!
//! FIPS mode itself comes from the host: the kernel flag activates the
//! validated OpenSSL provider and the system crypto policy, and this process
//! never enables a provider on its own. What it does is report the two signals
//! at startup and, when [`REQUIRE_FIPS_ENV`] is set, refuse to serve unless
//! both are present.
//!
//! The signals, the variable and the messages are those of praxis's
//! `praxis_tls::provider`; once a praxis release carries that module, this one
//! can delegate to it.
//!
//! [`CryptoProvider`]: rustls::crypto::CryptoProvider

use rustls::crypto::CryptoProvider;
use tracing::{debug, info};

use crate::error::ExtProcError;

/// Environment variable that makes FIPS mode a hard requirement.
///
/// Set it to `1`, `true`, `yes` or `on` (case-insensitive) and the server
/// refuses to serve unless [`Status::unmet`] is empty. It is a check, never a
/// switch: nothing here turns FIPS mode on.
pub const REQUIRE_FIPS_ENV: &str = "PRAXIS_REQUIRE_FIPS";

/// Name of the provider compiled into this build.
pub const PROVIDER_NAME: &str = "openssl";

/// Path of the kernel's FIPS mode flag.
const KERNEL_FIPS_FLAG: &str = "/proc/sys/crypto/fips_enabled";

// -----------------------------------------------------------------------------
// Status
// -----------------------------------------------------------------------------

/// What the process knows about FIPS once the provider is installed.
///
/// Two independent signals, kept apart so a log line says which one is
/// missing: the installed provider's own view of whether every primitive it
/// offers is FIPS approved, and the kernel's FIPS mode, which on Red Hat
/// Enterprise Linux is what activates the validated OpenSSL provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// Whether the installed provider reports every cipher suite, key exchange
    /// and signature algorithm as FIPS approved (rustls' `CryptoProvider::fips`,
    /// which the OpenSSL provider answers from
    /// `EVP_default_properties_is_fips_enabled`).
    pub provider_fips: bool,
    /// Whether the kernel is in FIPS mode, from `/proc/sys/crypto/fips_enabled`;
    /// `None` where that file does not exist (a non-Linux host, or a container
    /// without `/proc`).
    pub kernel_fips: Option<bool>,
}

impl Status {
    /// Whether FIPS mode is in effect: both signals present.
    #[must_use]
    pub fn active(self) -> bool {
        self.provider_fips && self.kernel_fips == Some(true)
    }

    /// Why FIPS mode is not in effect, one reason per missing signal. Empty
    /// when it is.
    #[must_use]
    pub fn unmet(self) -> Vec<&'static str> {
        let mut reasons = Vec::new();
        if !self.provider_fips {
            reasons
                .push("the OpenSSL provider does not report FIPS-approved algorithms (is the fips provider active?)");
        }
        match self.kernel_fips {
            Some(true) => {},
            Some(false) => reasons.push("the kernel is not in FIPS mode (/proc/sys/crypto/fips_enabled is 0)"),
            None => reasons.push("the kernel FIPS flag cannot be read (/proc/sys/crypto/fips_enabled)"),
        }
        reasons
    }

    /// Fail closed: when [`REQUIRE_FIPS_ENV`] is set and FIPS mode is not in
    /// effect, an error naming every missing signal.
    ///
    /// # Errors
    ///
    /// Returns [`ExtProcError::Crypto`] with the reasons from [`Status::unmet`].
    pub fn require(self) -> Result<(), ExtProcError> {
        self.require_if(required())
    }

    /// [`Status::require`] with the requirement decided by the caller.
    fn require_if(self, required: bool) -> Result<(), ExtProcError> {
        if !required {
            return Ok(());
        }
        let unmet = self.unmet();
        if unmet.is_empty() {
            return Ok(());
        }
        Err(ExtProcError::Crypto(format!(
            "{REQUIRE_FIPS_ENV} is set but FIPS mode is not in effect: {}",
            unmet.join("; ")
        )))
    }
}

/// Whether this deployment requires FIPS mode; see [`REQUIRE_FIPS_ENV`].
#[must_use]
pub fn required() -> bool {
    std::env::var(REQUIRE_FIPS_ENV).is_ok_and(|value| is_truthy(&value))
}

/// The affirmative spellings [`REQUIRE_FIPS_ENV`] accepts.
fn is_truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

// -----------------------------------------------------------------------------
// Install
// -----------------------------------------------------------------------------

/// Install the OpenSSL-backed provider as the process-wide default, read the
/// FIPS signals and log them.
///
/// Must run before anything builds a filter registry, a subrequest client or a
/// TLS configuration: rustls has no built-in fallback in this build, so a
/// provider that is not installed here is not installed at all. Idempotent: a
/// provider installed earlier (by a test, say) stays, since the first caller
/// wins.
///
/// # Errors
///
/// Returns [`ExtProcError::Crypto`] when no provider is installed afterwards.
pub fn install() -> Result<Status, ExtProcError> {
    if rustls_openssl::default_provider().install_default().is_err() {
        debug!("a crypto provider was already installed; keeping it");
    }
    let Some(provider) = CryptoProvider::get_default() else {
        return Err(ExtProcError::Crypto(format!(
            "failed to install the {PROVIDER_NAME} crypto provider"
        )));
    };
    let status = Status {
        provider_fips: provider.fips(),
        kernel_fips: std::fs::read_to_string(KERNEL_FIPS_FLAG)
            .ok()
            .and_then(|contents| kernel_fips_from(&contents)),
    };
    info!(
        provider = PROVIDER_NAME,
        provider_fips = status.provider_fips,
        kernel_fips = ?status.kernel_fips,
        fips_required = required(),
        "installed rustls crypto provider"
    );
    Ok(status)
}

/// Interpret the contents of the kernel's FIPS flag.
///
/// The kernel writes a single digit and a newline; anything else is unknown
/// rather than "off", so an unexpected file never reads as a claim either way.
fn kernel_fips_from(contents: &str) -> Option<bool> {
    match contents.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests")]
mod tests {
    use super::*;

    /// The status of a FIPS host.
    const FIPS_HOST: Status = Status {
        provider_fips: true,
        kernel_fips: Some(true),
    };

    #[test]
    fn fips_mode_needs_both_signals() {
        assert!(FIPS_HOST.active(), "provider and kernel both report FIPS");
        assert!(FIPS_HOST.unmet().is_empty(), "nothing is missing on a FIPS host");
        let neither = Status {
            provider_fips: false,
            kernel_fips: Some(false),
        };
        assert!(!neither.active(), "no signal, no FIPS mode");
        assert_eq!(neither.unmet().len(), 2, "both missing signals are named");
    }

    #[test]
    fn each_missing_signal_is_named() {
        for (status, missing) in [
            (
                Status {
                    provider_fips: false,
                    ..FIPS_HOST
                },
                "OpenSSL provider",
            ),
            (
                Status {
                    kernel_fips: Some(false),
                    ..FIPS_HOST
                },
                "not in FIPS mode",
            ),
            (
                Status {
                    kernel_fips: None,
                    ..FIPS_HOST
                },
                "cannot be read",
            ),
        ] {
            assert!(!status.active(), "one missing signal means no FIPS mode: {status:?}");
            let unmet = status.unmet();
            assert_eq!(unmet.len(), 1, "exactly the missing signal is named: {unmet:?}");
            let reason = unmet.first().copied().expect("one reason");
            assert!(reason.contains(missing), "the reason names the signal: {reason}");
        }
    }

    #[test]
    fn requirement_fails_closed_only_when_set_and_unmet() {
        let off = Status {
            provider_fips: false,
            kernel_fips: Some(false),
        };
        assert!(off.require_if(false).is_ok(), "not required: serve regardless");
        assert!(FIPS_HOST.require_if(true).is_ok(), "required and met: serve");
        let err = off.require_if(true).expect_err("required and unmet: refuse");
        let message = err.to_string();
        assert!(
            message.contains("PRAXIS_REQUIRE_FIPS is set but FIPS mode is not in effect"),
            "the message names the variable and the state: {message}"
        );
        assert!(
            message.contains("provider") && message.contains("kernel"),
            "and every missing signal: {message}"
        );
    }

    #[test]
    fn the_variable_accepts_the_usual_affirmatives_only() {
        for value in ["1", "true", "TRUE", "yes", " on "] {
            assert!(is_truthy(value), "{value:?} requires FIPS");
        }
        for value in ["", "0", "false", "no", "off", "maybe"] {
            assert!(!is_truthy(value), "{value:?} does not");
        }
    }

    #[test]
    fn the_kernel_flag_is_a_single_digit() {
        assert_eq!(kernel_fips_from("1\n"), Some(true), "1 is FIPS mode");
        assert_eq!(kernel_fips_from("0\n"), Some(false), "0 is not");
        assert_eq!(kernel_fips_from("garbage"), None, "anything else is unknown");
    }

    #[test]
    fn install_is_idempotent_and_leaves_a_provider_installed() {
        let first = install().expect("install");
        let second = install().expect("a second install keeps the first provider");
        assert_eq!(first, second, "the signals do not change between calls");
        assert!(CryptoProvider::get_default().is_some(), "a provider is installed");
    }
}
