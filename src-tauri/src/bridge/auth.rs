//! JWT-based authentication for IDE Bridge connections.
//!
//! Uses HS256 with a random per-instance secret, tokens expire after 24 hours.

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// Subject — always "ide-bridge"
    sub: String,
    /// Issued-at (Unix timestamp)
    iat: u64,
    /// Expiry (Unix timestamp)
    exp: u64,
}

/// Generate a signed HS256 JWT valid for 24 hours.
pub fn generate_token(secret: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let claims = Claims {
        sub: "ide-bridge".to_string(),
        iat: now,
        exp: now + 86_400, // 24 h
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap_or_default()
}

/// Verify a JWT against `secret`.  Returns `true` when signature and expiry are valid.
pub fn verify_token(token: &str, secret: &str) -> bool {
    let mut validation = Validation::default();
    validation.set_required_spec_claims(&["sub", "exp"]);

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let secret = "super-secret-key";
        let token = generate_token(secret);
        assert!(verify_token(&token, secret));
    }

    #[test]
    fn wrong_secret_rejected() {
        let token = generate_token("secret-a");
        assert!(!verify_token(&token, "secret-b"));
    }
}
