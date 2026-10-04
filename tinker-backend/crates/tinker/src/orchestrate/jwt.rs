//! Compact HS256 JWTs with session, user, and workspace claims.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tinker_protocol::{Jwt, SessionId, UserId, WorkspaceId};

type HmacSha256 = Hmac<Sha256>;

/// Claims minted at approve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claims {
    /// Session id.
    pub session_id: SessionId,
    /// User id.
    pub user_id: UserId,
    /// Workspace id.
    pub workspace_id: WorkspaceId,
    /// Expiry unix seconds.
    pub exp: u64,
    /// Issued-at unix seconds.
    pub iat: u64,
}

/// Why verify failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JwtVerifyError {
    /// Compact form or JSON claims failed.
    Form,
    /// HMAC mismatch.
    Signature,
    /// `exp` is in the past.
    Expired,
}

/// Mint a compact HS256 JWT.
#[must_use]
pub fn mint(secret: &[u8], claims: &Claims) -> Jwt {
    let header = b64(b"{\"alg\":\"HS256\",\"typ\":\"JWT\"}");
    let payload = b64(
        format!(
            "{{\"session_id\":\"{}\",\"user_id\":\"{}\",\"workspace_id\":\"{}\",\"exp\":{},\"iat\":{}}}",
            claims.session_id, claims.user_id, claims.workspace_id, claims.exp, claims.iat
        )
        .as_bytes(),
    );
    let signing = format!("{header}.{payload}");
    let sig = b64(&hmac_sha256(secret, signing.as_bytes()));
    Jwt::new(&format!("{signing}.{sig}")).expect("compact jwt")
}

/// Verify signature, expiry, and required claims.
///
/// # Errors
///
/// Returns [`JwtVerifyError`] when the token is malformed, forged, or expired.
pub fn verify(secret: &[u8], token: &str, now: u64) -> Result<Claims, JwtVerifyError> {
    let jwt = Jwt::new(token).map_err(|_| JwtVerifyError::Form)?;
    let raw = jwt.as_str();
    let mut parts = raw.split('.');
    let header = parts.next().expect("three segments");
    let payload = parts.next().expect("three segments");
    let sig = parts.next().expect("three segments");
    let signing = format!("{header}.{payload}");
    let expected = hmac_sha256(secret, signing.as_bytes());
    let got = unb64(sig).ok_or(JwtVerifyError::Form)?;
    if !ct_eq(&expected, &got) {
        return Err(JwtVerifyError::Signature);
    }
    let json = unb64(payload).ok_or(JwtVerifyError::Form)?;
    let text = String::from_utf8(json).map_err(|_| JwtVerifyError::Form)?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| JwtVerifyError::Form)?;
    let session_id = str_claim(&v, "session_id").ok_or(JwtVerifyError::Form)?;
    let user_id = str_claim(&v, "user_id").ok_or(JwtVerifyError::Form)?;
    let workspace_id = str_claim(&v, "workspace_id").ok_or(JwtVerifyError::Form)?;
    let exp = v
        .get("exp")
        .and_then(serde_json::Value::as_u64)
        .ok_or(JwtVerifyError::Form)?;
    let iat = v
        .get("iat")
        .and_then(serde_json::Value::as_u64)
        .ok_or(JwtVerifyError::Form)?;
    if now >= exp {
        return Err(JwtVerifyError::Expired);
    }
    Ok(Claims {
        session_id: SessionId::new(session_id).map_err(|_| JwtVerifyError::Form)?,
        user_id: UserId::new(user_id).map_err(|_| JwtVerifyError::Form)?,
        workspace_id: WorkspaceId::new(workspace_id).map_err(|_| JwtVerifyError::Form)?,
        exp,
        iat,
    })
}

fn str_claim<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(serde_json::Value::as_str)
}

fn hmac_sha256(secret: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.decode(s).ok()
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0123456789abcdef0123456789abcdef";

    fn sample() -> Claims {
        Claims {
            session_id: SessionId::new(HEX).expect("s"),
            user_id: UserId::new(HEX).expect("u"),
            workspace_id: WorkspaceId::new(HEX).expect("w"),
            exp: 2_000,
            iat: 1_000,
        }
    }

    #[test]
    fn round_trip_and_failures() {
        let secret = b"unit-test-secret";
        let token = mint(secret, &sample());
        let c = verify(secret, token.as_str(), 1_500).expect("ok");
        assert_eq!(c.exp, 2_000);
        assert_eq!(
            verify(secret, token.as_str(), 2_000),
            Err(JwtVerifyError::Expired)
        );
        assert_eq!(
            verify(b"other", token.as_str(), 1_500),
            Err(JwtVerifyError::Signature)
        );
        assert_eq!(verify(secret, "a.b", 1_500), Err(JwtVerifyError::Form));
        assert_eq!(verify(secret, "a.b.!", 1_500), Err(JwtVerifyError::Form));
        let mut bad = token.as_str().to_owned();
        bad.push_str(".x");
        assert_eq!(verify(secret, &bad, 1_500), Err(JwtVerifyError::Form));
        let claims_missing = mint_raw(secret, b"{\"alg\":\"HS256\"}", b"{}");
        assert_eq!(
            verify(secret, &claims_missing, 1),
            Err(JwtVerifyError::Form)
        );
        let no_user =
            format!("{{\"session_id\":\"{HEX}\",\"workspace_id\":\"{HEX}\",\"exp\":9,\"iat\":1}}");
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", no_user.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let no_workspace =
            format!("{{\"session_id\":\"{HEX}\",\"user_id\":\"{HEX}\",\"exp\":9,\"iat\":1}}");
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", no_workspace.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let no_exp = format!(
            "{{\"session_id\":\"{HEX}\",\"user_id\":\"{HEX}\",\"workspace_id\":\"{HEX}\",\"iat\":1}}"
        );
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", no_exp.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let bad_user = format!(
            "{{\"session_id\":\"{HEX}\",\"user_id\":\"nope\",\"workspace_id\":\"{HEX}\",\"exp\":9,\"iat\":1}}"
        );
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", bad_user.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let bad_workspace = format!(
            "{{\"session_id\":\"{HEX}\",\"user_id\":\"{HEX}\",\"workspace_id\":\"nope\",\"exp\":9,\"iat\":1}}"
        );
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", bad_workspace.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let h = b64(b"{\"alg\":\"HS256\"}");
        let p = "!!!";
        let signing = format!("{h}.{p}");
        let sig = b64(&hmac_sha256(secret, signing.as_bytes()));
        assert_eq!(
            verify(secret, &format!("{signing}.{sig}"), 1),
            Err(JwtVerifyError::Form)
        );
        let not_utf8_payload = mint_raw(secret, b"{\"alg\":\"HS256\"}", &[0xff, 0xfe]);
        assert_eq!(
            verify(secret, &not_utf8_payload, 1),
            Err(JwtVerifyError::Form)
        );
        let not_json = mint_raw(secret, b"{\"alg\":\"HS256\"}", b"{");
        assert_eq!(verify(secret, &not_json, 1), Err(JwtVerifyError::Form));
        let bad_id = format!(
            "{{\"session_id\":\"nope\",\"user_id\":\"{HEX}\",\"workspace_id\":\"{HEX}\",\"exp\":9,\"iat\":1}}"
        );
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", bad_id.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let no_iat = format!(
            "{{\"session_id\":\"{HEX}\",\"user_id\":\"{HEX}\",\"workspace_id\":\"{HEX}\",\"exp\":9}}"
        );
        assert_eq!(
            verify(
                secret,
                &mint_raw(secret, b"{\"alg\":\"HS256\"}", no_iat.as_bytes()),
                1
            ),
            Err(JwtVerifyError::Form)
        );
        let token = mint(secret, &sample());
        let mut parts = token.as_str().split('.');
        let h = parts.next().expect("h");
        let p = parts.next().expect("p");
        let short = mint_raw(secret, b"x", b"y");
        let short_sig = short.split('.').nth(2).expect("s");
        assert_eq!(
            verify(secret, &format!("{h}.{p}.{short_sig}"), 1_500),
            Err(JwtVerifyError::Signature)
        );
        assert!(!ct_eq(b"ab", b"a"));
        assert!(ct_eq(b"ab", b"ab"));
    }

    fn mint_raw(secret: &[u8], header: &[u8], payload: &[u8]) -> String {
        let h = b64(header);
        let p = b64(payload);
        let signing = format!("{h}.{p}");
        let sig = b64(&hmac_sha256(secret, signing.as_bytes()));
        format!("{signing}.{sig}")
    }
}
