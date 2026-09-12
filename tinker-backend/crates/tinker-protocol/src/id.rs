//! Wire identifiers and display names.

use core::fmt;

/// 16-byte id as 32 lowercase hex characters (`RequestId`, `SessionId`).
fn parse_hex16(raw: &str) -> Result<String, HexIdError> {
    if raw.len() != 32 {
        return Err(HexIdError::Length);
    }
    if !raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(HexIdError::Charset);
    }
    Ok(raw.to_owned())
}

/// Why a hex id was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HexIdError {
    /// Not 32 characters.
    Length,
    /// A character is not lowercase hex.
    Charset,
}

impl fmt::Display for HexIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => f.write_str("id must be 32 lowercase hex characters"),
            Self::Charset => f.write_str("id must be lowercase hex"),
        }
    }
}

macro_rules! hex_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Parse lowercase hex.
            ///
            /// # Errors
            ///
            /// Returns [`HexIdError`] when the value is not 32 lowercase hex characters.
            pub fn new(raw: &str) -> Result<Self, HexIdError> {
                Ok(Self(parse_hex16(raw)?))
            }

            /// Hex characters.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

hex_id!(
    RequestId,
    "Internal access-request id (16 bytes, lowercase hex)."
);
hex_id!(SessionId, "Session id (16 bytes, lowercase hex).");

/// Wait token: 32 bytes, unpadded base64url (43 characters).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitToken(String);

/// Why a [`WaitToken`] was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitTokenError {
    /// Not 43 characters.
    Length,
    /// A character is outside the unpadded base64url alphabet.
    Charset,
}

impl WaitToken {
    /// Parse an unpadded base64url token.
    ///
    /// # Errors
    ///
    /// Returns [`WaitTokenError`] when the length or alphabet is wrong.
    pub fn new(raw: &str) -> Result<Self, WaitTokenError> {
        if raw.len() != 43 {
            return Err(WaitTokenError::Length);
        }
        if !raw.bytes().all(is_base64url) {
            return Err(WaitTokenError::Charset);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Token characters.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WaitToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for WaitTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => f.write_str("wait token must be 43 base64url characters"),
            Self::Charset => f.write_str("wait token must be unpadded base64url"),
        }
    }
}

fn is_base64url(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_')
}

/// Compact JWT. `Debug` omits the token.
#[derive(Clone, Eq, PartialEq)]
pub struct Jwt(String);

/// Why a [`Jwt`] was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JwtError {
    /// Not three non-empty `.`-separated segments.
    Form,
}

impl Jwt {
    /// Parse compact JWT form `header.payload.signature`.
    ///
    /// # Errors
    ///
    /// Returns [`JwtError::Form`] when there are not three non-empty segments.
    pub fn new(raw: &str) -> Result<Self, JwtError> {
        let mut parts = raw.split('.');
        let a = parts.next().ok_or(JwtError::Form)?;
        let b = parts.next().ok_or(JwtError::Form)?;
        let c = parts.next().ok_or(JwtError::Form)?;
        if parts.next().is_some() || a.is_empty() || b.is_empty() || c.is_empty() {
            return Err(JwtError::Form);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Compact serialization.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Jwt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Jwt").field(&"<redacted>").finish()
    }
}

impl fmt::Display for JwtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JWT must be three non-empty dot-separated segments")
    }
}

/// Visitor display name: 1–64 characters after trimming whitespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayName(String);

/// Why a [`DisplayName`] was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayNameError {
    /// Empty after trim.
    Empty,
    /// Longer than 64 characters after trim.
    TooLong,
}

impl DisplayName {
    /// Trim and bound a display name.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayNameError`] when the trimmed name is empty or longer than 64 characters.
    pub fn new(raw: &str) -> Result<Self, DisplayNameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        if name.chars().count() > 64 {
            return Err(DisplayNameError::TooLong);
        }
        Ok(Self(name.to_owned()))
    }

    /// Trimmed name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for DisplayNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("display name is empty"),
            Self::TooLong => f.write_str("display name is longer than 64 characters"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_ids() {
        let ok = "0123456789abcdef0123456789abcdef";
        let id = RequestId::new(ok).expect("id");
        assert_eq!(id.as_str(), ok);
        assert_eq!(id.to_string(), ok);
        let sid = SessionId::new(ok).expect("sid");
        assert_eq!(sid.as_str(), ok);
        assert_eq!(RequestId::new("short"), Err(HexIdError::Length));
        assert_eq!(RequestId::new(&"g".repeat(32)), Err(HexIdError::Charset));
        assert_eq!(RequestId::new(&"A".repeat(32)), Err(HexIdError::Charset));
        assert_eq!(
            HexIdError::Length.to_string(),
            "id must be 32 lowercase hex characters"
        );
        assert_eq!(HexIdError::Charset.to_string(), "id must be lowercase hex");
    }

    #[test]
    fn wait_token() {
        let ok = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmno-_";
        assert_eq!(ok.len(), 43);
        let t = WaitToken::new(ok).expect("tok");
        assert_eq!(t.as_str(), ok);
        assert_eq!(t.to_string(), ok);
        assert_eq!(WaitToken::new("short"), Err(WaitTokenError::Length));
        assert_eq!(
            WaitToken::new(&"=".repeat(43)),
            Err(WaitTokenError::Charset)
        );
        assert!(WaitTokenError::Length.to_string().contains("43"));
        assert!(WaitTokenError::Charset.to_string().contains("base64url"));
    }

    #[test]
    fn jwt_redacted() {
        let jwt = Jwt::new("aaa.bbb.ccc").expect("jwt");
        assert_eq!(jwt.as_str(), "aaa.bbb.ccc");
        let dbg = format!("{jwt:?}");
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains("aaa"));
        assert_eq!(Jwt::new("aaa.bbb"), Err(JwtError::Form));
        assert_eq!(Jwt::new("aaa.bbb.ccc.ddd"), Err(JwtError::Form));
        assert_eq!(Jwt::new(".bbb.ccc"), Err(JwtError::Form));
        assert_eq!(Jwt::new("aaa..ccc"), Err(JwtError::Form));
        assert_eq!(Jwt::new("aaa.bbb."), Err(JwtError::Form));
        assert!(JwtError::Form.to_string().contains("three"));
    }

    #[test]
    fn display_name() {
        let n = DisplayName::new("  Ada  ").expect("name");
        assert_eq!(n.as_str(), "Ada");
        assert_eq!(n.to_string(), "Ada");
        assert_eq!(DisplayName::new("   "), Err(DisplayNameError::Empty));
        assert_eq!(DisplayName::new(""), Err(DisplayNameError::Empty));
        let long: String = "é".repeat(65);
        assert_eq!(DisplayName::new(&long), Err(DisplayNameError::TooLong));
        assert_eq!(
            DisplayName::new(&"a".repeat(64))
                .expect("64")
                .as_str()
                .len(),
            64
        );
        assert!(DisplayNameError::Empty.to_string().contains("empty"));
        assert!(DisplayNameError::TooLong.to_string().contains("64"));
    }
}
