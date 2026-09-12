//! Verification of the HMAC tickets Laravel mints.
//!
//! The construction is fixed by the PHP side (`App\Services\GatewayTicketService`)
//! and must match it byte for byte:
//!
//! ```text
//! ticket  = base64url(json(payload)) "." base64url(hmac_sha256(base64url(json(payload)), secret))
//! payload = {"v":1,"typ":…,"sid":…,"mid":…,"rid":…,"scope":…,"jti":…,"iat":…,"exp":…}
//! ```
//!
//! Note the signature covers the *encoded* payload, not the raw JSON — signing
//! the decoded form would let two different encodings of the same object share
//! a signature.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Allowance for clock drift between the app and the gateway, matching the
/// PHP side. Without it a gateway a few seconds ahead would reject tickets
/// that were valid when they were minted.
const CLOCK_SKEW_SECONDS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TicketPayload {
    pub v: u8,
    /// `session` for a browser, `machine` for an agent, `gateway` for a call
    /// the gateway makes back to the control plane.
    pub typ: String,
    /// Session uuid. Absent on a machine ticket, which is not tied to one.
    #[serde(default)]
    pub sid: Option<String>,
    /// Machine uuid.
    #[serde(default)]
    pub mid: Option<String>,
    /// Rental uuid.
    #[serde(default)]
    pub rid: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TicketError {
    #[error("the ticket is not in the expected form")]
    Malformed,
    #[error("the ticket signature does not verify")]
    BadSignature,
    #[error("the ticket has expired")]
    Expired,
    #[error("the ticket is not valid for this endpoint")]
    WrongType,
}

/// Verify a ticket and return its payload.
///
/// `now` is passed in rather than read here so expiry is testable without
/// waiting, and so a caller can reason about which clock is being used.
pub fn verify(ticket: &str, secret: &[u8], now: i64) -> Result<TicketPayload, TicketError> {
    let (payload_b64, signature_b64) = ticket.split_once('.').ok_or(TicketError::Malformed)?;

    if payload_b64.is_empty() || signature_b64.is_empty() {
        return Err(TicketError::Malformed);
    }

    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| TicketError::BadSignature)?;
    mac.update(payload_b64.as_bytes());

    let provided = base64url_decode(signature_b64).ok_or(TicketError::Malformed)?;

    // Constant time, via the MAC's own comparison.
    mac.verify_slice(&provided).map_err(|_| TicketError::BadSignature)?;

    let decoded = base64url_decode(payload_b64).ok_or(TicketError::Malformed)?;
    let payload: TicketPayload =
        serde_json::from_slice(&decoded).map_err(|_| TicketError::Malformed)?;

    if payload.exp < now - CLOCK_SKEW_SECONDS {
        return Err(TicketError::Expired);
    }

    Ok(payload)
}

/// Verify and additionally require a particular `typ`, so a browser ticket
/// cannot be presented at the agent endpoint or vice versa.
pub fn verify_typed(
    ticket: &str,
    secret: &[u8],
    now: i64,
    expected: &str,
) -> Result<TicketPayload, TicketError> {
    let payload = verify(ticket, secret, now)?;

    if payload.typ != expected {
        return Err(TicketError::WrongType);
    }

    Ok(payload)
}

/// Mint a ticket. Used by the gateway to authenticate its callbacks to the
/// control plane, which verifies them with the same secret.
pub fn mint(payload: &TicketPayload, secret: &[u8]) -> String {
    let json = serde_json::to_vec(payload).expect("ticket payload is serialisable");
    let payload_b64 = base64url_encode(&json);

    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload_b64.as_bytes());

    format!("{payload_b64}.{}", base64url_encode(&mac.finalize().into_bytes()))
}

fn base64url_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn base64url_decode(value: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"gateway-secret";

    fn payload(typ: &str, exp: i64) -> TicketPayload {
        TicketPayload {
            v: 1,
            typ: typ.to_string(),
            sid: Some("session-1".into()),
            mid: Some("machine-1".into()),
            rid: Some("rental-1".into()),
            scope: Some("webrtc".into()),
            iat: 1_000,
            exp,
        }
    }

    #[test]
    fn round_trips_a_minted_ticket() {
        let ticket = mint(&payload("session", 2_000), SECRET);
        let verified = verify(&ticket, SECRET, 1_500).unwrap();

        assert_eq!(verified.typ, "session");
        assert_eq!(verified.sid.as_deref(), Some("session-1"));
        assert_eq!(verified.mid.as_deref(), Some("machine-1"));
    }

    #[test]
    fn rejects_a_ticket_signed_with_another_secret() {
        let ticket = mint(&payload("session", 2_000), b"someone-elses-secret");

        assert_eq!(verify(&ticket, SECRET, 1_500), Err(TicketError::BadSignature));
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let ticket = mint(&payload("session", 2_000), SECRET);
        let (_, signature) = ticket.split_once('.').unwrap();

        // Re-encode a different payload against the original signature.
        let forged = mint(&payload("session", 9_999), SECRET);
        let (forged_payload, _) = forged.split_once('.').unwrap();

        assert_eq!(
            verify(&format!("{forged_payload}.{signature}"), SECRET, 1_500),
            Err(TicketError::BadSignature)
        );
    }

    #[test]
    fn rejects_an_expired_ticket_but_allows_for_clock_drift() {
        let ticket = mint(&payload("session", 2_000), SECRET);

        // A gateway slightly ahead of the app must not reject a live ticket.
        assert!(verify(&ticket, SECRET, 2_020).is_ok());
        assert_eq!(verify(&ticket, SECRET, 2_100), Err(TicketError::Expired));
    }

    #[test]
    fn refuses_a_ticket_meant_for_another_endpoint() {
        // A browser's session ticket must not open the agent's channel.
        let ticket = mint(&payload("session", 2_000), SECRET);

        assert_eq!(
            verify_typed(&ticket, SECRET, 1_500, "machine"),
            Err(TicketError::WrongType)
        );
        assert!(verify_typed(&ticket, SECRET, 1_500, "session").is_ok());
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(verify("", SECRET, 0), Err(TicketError::Malformed));
        assert_eq!(verify("no-dot", SECRET, 0), Err(TicketError::Malformed));
        assert_eq!(verify(".", SECRET, 0), Err(TicketError::Malformed));
        assert_eq!(verify("a.", SECRET, 0), Err(TicketError::Malformed));
        assert_eq!(verify("!!!.!!!", SECRET, 0), Err(TicketError::Malformed));
    }
}

/// Cross-language contract tests.
///
/// A ticket minted by the PHP control plane, pinned here verbatim. This is the
/// one thing about the ticket format that cannot be caught by testing either
/// side alone: both could be internally consistent and still disagree on what
/// exactly gets signed. If the PHP side changes how it builds a ticket, this
/// test is what notices.
#[cfg(test)]
mod cross_language {
    use super::*;

    /// Minted by `App\Services\GatewayTicketService` with the secret below.
    const PHP_TICKET: &str = "eyJ2IjoxLCJ0eXAiOiJzZXNzaW9uIiwic2lkIjoic2Vzc2lvbi0xIiwibWlkIjoibWFjaGluZS0xIiwicmlkIjoicmVudGFsLTEiLCJzY29wZSI6IndlYnJ0YyIsImp0aSI6IkFMYTJHTVltaU5PallodHUiLCJpYXQiOjE3ODg4Mjk4NTYsImV4cCI6MTc4ODgzMzQ1Nn0.ra8nn2NqUkdfIZ39_hb_J3PmXSngG8pzIDcxm6LxMDI";
    const PHP_SECRET: &[u8] = b"gateway-secret";

    /// A moment inside the pinned ticket's validity window.
    const DURING: i64 = 1_788_830_000;

    #[test]
    fn accepts_a_ticket_minted_by_the_control_plane() {
        let payload = verify(PHP_TICKET, PHP_SECRET, DURING)
            .expect("Rust and PHP disagree about how a ticket is signed");

        assert_eq!(payload.v, 1);
        assert_eq!(payload.typ, "session");
        assert_eq!(payload.sid.as_deref(), Some("session-1"));
        assert_eq!(payload.mid.as_deref(), Some("machine-1"));
        assert_eq!(payload.rid.as_deref(), Some("rental-1"));
        assert_eq!(payload.scope.as_deref(), Some("webrtc"));
    }

    #[test]
    fn tolerates_the_unknown_jti_field() {
        // PHP adds a nonce the Rust struct does not model. Deserialisation
        // must not care, or every ticket would be rejected.
        assert!(verify(PHP_TICKET, PHP_SECRET, DURING).is_ok());
    }

    #[test]
    fn still_rejects_the_php_ticket_under_the_wrong_secret() {
        assert_eq!(
            verify(PHP_TICKET, b"not-the-secret", DURING),
            Err(TicketError::BadSignature)
        );
    }

    #[test]
    fn honours_the_php_expiry() {
        assert_eq!(verify(PHP_TICKET, PHP_SECRET, 1_788_900_000), Err(TicketError::Expired));
    }
}
