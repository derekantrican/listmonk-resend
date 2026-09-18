//! Verification of Resend (Svix) webhook signatures.
//!
//! The signed content is `{svix-id}.{svix-timestamp}.{raw body}`, signed with HMAC-SHA256 using the
//! base64-decoded part of the `whsec_...` secret. The `svix-signature` header holds one or more
//! space separated `v1,{base64 signature}` entries.

use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Maximum allowed difference between the webhook timestamp and now.
const TOLERANCE_SECS: i64 = 5 * 60;

#[derive(Debug, PartialEq)]
pub enum SignatureError {
    InvalidSecret,
    InvalidTimestamp,
    TimestampOutOfTolerance,
    NoMatchingSignature,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn verify(
    secret: &str,
    id: &str,
    timestamp: &str,
    signatures: &str,
    body: &[u8],
    now: i64,
) -> Result<(), SignatureError> {
    let key = STANDARD
        .decode(secret.strip_prefix("whsec_").unwrap_or(secret))
        .map_err(|_| SignatureError::InvalidSecret)?;
    let ts: i64 = timestamp
        .parse()
        .map_err(|_| SignatureError::InvalidTimestamp)?;
    if (now - ts).abs() > TOLERANCE_SECS {
        return Err(SignatureError::TimestampOutOfTolerance);
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|_| SignatureError::InvalidSecret)?;
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);

    let matches = signatures
        .split_whitespace()
        .filter_map(|entry| entry.strip_prefix("v1,"))
        .filter_map(|sig| STANDARD.decode(sig).ok())
        .any(|sig| mac.clone().verify_slice(&sig).is_ok());
    if matches {
        Ok(())
    } else {
        Err(SignatureError::NoMatchingSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";

    fn sign(id: &str, timestamp: &str, body: &[u8]) -> String {
        let key = STANDARD.decode(SECRET.strip_prefix("whsec_").unwrap()).unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).unwrap();
        mac.update(format!("{}.{}.", id, timestamp).as_bytes());
        mac.update(body);
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn test_valid_signature() {
        let body = br#"{"type":"email.bounced"}"#;
        let sig = sign("msg_1", "1700000000", body);
        assert_eq!(verify(SECRET, "msg_1", "1700000000", &sig, body, 1700000010), Ok(()));
    }

    #[test]
    fn test_valid_signature_among_several() {
        let body = b"{}";
        let header = format!("v1,AAAA {}", sign("msg_1", "1700000000", body));
        assert_eq!(verify(SECRET, "msg_1", "1700000000", &header, body, 1700000000), Ok(()));
    }

    #[test]
    fn test_tampered_body() {
        let sig = sign("msg_1", "1700000000", b"{}");
        assert_eq!(
            verify(SECRET, "msg_1", "1700000000", &sig, b"{\"a\":1}", 1700000000),
            Err(SignatureError::NoMatchingSignature)
        );
    }

    #[test]
    fn test_old_timestamp() {
        let sig = sign("msg_1", "1700000000", b"{}");
        assert_eq!(
            verify(SECRET, "msg_1", "1700000000", &sig, b"{}", 1700001000),
            Err(SignatureError::TimestampOutOfTolerance)
        );
    }

    #[test]
    fn test_invalid_secret() {
        assert_eq!(
            verify("whsec_!!!", "id", "1700000000", "v1,AAAA", b"{}", 1700000000),
            Err(SignatureError::InvalidSecret)
        );
    }
}
