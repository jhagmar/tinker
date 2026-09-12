//! HTTP JSON bodies, error codes, and the public/admin route table.

use core::fmt;

use crate::id::{DisplayName, DisplayNameError, Jwt, RequestId, SessionId};

/// Default session TTL (seconds).
pub const DEFAULT_TTL_SECONDS: u32 = 3600;

/// Maximum session TTL (7 days).
pub const MAX_TTL_SECONDS: u32 = 604_800;

/// Pending apply TTL (seconds).
pub const PENDING_TTL_SECONDS: u32 = 900;

/// Which listener a route is served on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Listener {
    /// Public socket.
    Public,
    /// Admin socket.
    Admin,
}

/// One HTTP-JSON route (WebSockets omitted).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpRoute {
    /// Fetch helper name (`apply`, `login`, …).
    pub name: &'static str,
    /// Public or admin listener.
    pub listener: Listener,
    /// HTTP method.
    pub method: &'static str,
    /// Path template (`{request_id}` placeholders).
    pub path: &'static str,
}

/// HTTP-JSON routes for codegen. Session and wait sockets are omitted.
pub const HTTP_ROUTES: &[HttpRoute] = &[
    HttpRoute {
        name: "apply",
        listener: Listener::Public,
        method: "POST",
        path: "/v1/access-requests",
    },
    HttpRoute {
        name: "getAccessRequest",
        listener: Listener::Public,
        method: "GET",
        path: "/v1/access-requests/{request_id}",
    },
    HttpRoute {
        name: "languages",
        listener: Listener::Public,
        method: "GET",
        path: "/v1/languages",
    },
    HttpRoute {
        name: "problems",
        listener: Listener::Public,
        method: "GET",
        path: "/v1/problems",
    },
    HttpRoute {
        name: "login",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/login",
    },
    HttpRoute {
        name: "logout",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/logout",
    },
    HttpRoute {
        name: "listAccessRequests",
        listener: Listener::Admin,
        method: "GET",
        path: "/v1/access-requests",
    },
    HttpRoute {
        name: "approve",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/access-requests/{request_id}/approve",
    },
    HttpRoute {
        name: "deny",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/access-requests/{request_id}/deny",
    },
    HttpRoute {
        name: "listSessions",
        listener: Listener::Admin,
        method: "GET",
        path: "/v1/sessions",
    },
    HttpRoute {
        name: "revoke",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/sessions/{session_id}/revoke",
    },
    HttpRoute {
        name: "extend",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/sessions/{session_id}/extend",
    },
    HttpRoute {
        name: "languages",
        listener: Listener::Admin,
        method: "GET",
        path: "/v1/languages",
    },
    HttpRoute {
        name: "setLanguageEnabled",
        listener: Listener::Admin,
        method: "POST",
        path: "/v1/languages/{id}/enabled",
    },
    HttpRoute {
        name: "problems",
        listener: Listener::Admin,
        method: "GET",
        path: "/v1/problems",
    },
];

/// Machine `error` field on `{ error, message }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// Memory budget cannot admit a session.
    Full,
    /// Language missing from the available set.
    UnavailableLanguage,
    /// Hello language disagrees with the live box.
    LanguageMismatch,
    /// Catalog stem is unknown.
    UnknownProblem,
    /// JWT or wait token failed.
    BadToken,
    /// Apply was denied.
    Denied,
    /// TTL elapsed.
    Expired,
    /// Sandbox or agent start failed.
    StartFailed,
    /// Request body or query failed validation.
    BadRequest,
}

impl ErrorCode {
    /// Wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::UnavailableLanguage => "unavailable_language",
            Self::LanguageMismatch => "language_mismatch",
            Self::UnknownProblem => "unknown_problem",
            Self::BadToken => "bad_token",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::StartFailed => "start_failed",
            Self::BadRequest => "bad_request",
        }
    }

    /// Parse a wire string.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCodeError`] when `raw` is not a known code.
    pub fn parse(raw: &str) -> Result<Self, ErrorCodeError> {
        match raw {
            "full" => Ok(Self::Full),
            "unavailable_language" => Ok(Self::UnavailableLanguage),
            "language_mismatch" => Ok(Self::LanguageMismatch),
            "unknown_problem" => Ok(Self::UnknownProblem),
            "bad_token" => Ok(Self::BadToken),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            "start_failed" => Ok(Self::StartFailed),
            "bad_request" => Ok(Self::BadRequest),
            _ => Err(ErrorCodeError::Unknown),
        }
    }
}

/// Unknown machine error code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCodeError {
    /// Not in the v1 set.
    Unknown,
}

impl fmt::Display for ErrorCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown error code")
    }
}

/// `{ error, message }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorBody {
    /// Machine code.
    pub error: ErrorCode,
    /// Human-readable next step.
    pub message: String,
}

impl ErrorBody {
    /// Construct an error payload. `message` is trimmed; empty becomes the code string.
    #[must_use]
    pub fn new(error: ErrorCode, message: &str) -> Self {
        let message = message.trim();
        let message = if message.is_empty() {
            error.as_str().to_owned()
        } else {
            message.to_owned()
        };
        Self { error, message }
    }
}

/// v1 `LanguageId`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LanguageId {
    /// Python.
    Python,
    /// JavaScript (Node stdlib).
    Javascript,
    /// TypeScript.
    Typescript,
    /// Java.
    Java,
    /// C++.
    Cpp,
    /// C#.
    Csharp,
    /// Rust.
    Rust,
    /// Go.
    Go,
}

impl LanguageId {
    /// Wire id.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Javascript => "javascript",
            Self::Typescript => "typescript",
            Self::Java => "java",
            Self::Cpp => "cpp",
            Self::Csharp => "csharp",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    /// All v1 language ids in spec order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Python,
            Self::Javascript,
            Self::Typescript,
            Self::Java,
            Self::Cpp,
            Self::Csharp,
            Self::Rust,
            Self::Go,
        ]
    }

    /// Parse a wire id.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageIdError`] when `raw` is not a v1 id.
    pub fn parse(raw: &str) -> Result<Self, LanguageIdError> {
        match raw {
            "python" => Ok(Self::Python),
            "javascript" => Ok(Self::Javascript),
            "typescript" => Ok(Self::Typescript),
            "java" => Ok(Self::Java),
            "cpp" => Ok(Self::Cpp),
            "csharp" => Ok(Self::Csharp),
            "rust" => Ok(Self::Rust),
            "go" => Ok(Self::Go),
            _ => Err(LanguageIdError::Unknown),
        }
    }
}

/// Unknown language id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageIdError {
    /// Not a v1 id.
    Unknown,
}

impl fmt::Display for LanguageIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown language id")
    }
}

/// Language file / available-language row.
#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    /// Language id.
    pub id: LanguageId,
    /// Image reference (digest-pinned at spawn).
    pub image: String,
    /// Memory cap string.
    pub memory: String,
    /// CPU allocation.
    pub cpus: f64,
    /// PID cap.
    pub pids: u32,
    /// Tmpfs size string.
    pub tmpfs: String,
    /// Operator enabled flag.
    pub enabled: bool,
}

/// Why [`Language::new`] failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageError {
    /// `cpus` is not a positive finite number.
    Cpus,
    /// A required string field is empty.
    Empty,
}

impl Language {
    /// Construct a language row.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageError`] when `cpus` is not finite and positive, or a string field is empty.
    pub fn new(
        id: LanguageId,
        image: &str,
        memory: &str,
        cpus: f64,
        pids: u32,
        tmpfs: &str,
        enabled: bool,
    ) -> Result<Self, LanguageError> {
        if !cpus.is_finite() || cpus <= 0.0 {
            return Err(LanguageError::Cpus);
        }
        if image.is_empty() || memory.is_empty() || tmpfs.is_empty() {
            return Err(LanguageError::Empty);
        }
        Ok(Self {
            id,
            image: image.to_owned(),
            memory: memory.to_owned(),
            cpus,
            pids,
            tmpfs: tmpfs.to_owned(),
            enabled,
        })
    }
}

/// Catalog list row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProblemSummary {
    /// Catalog stem.
    pub id: String,
    /// Description summary.
    pub description: String,
    /// Optional visualize id.
    pub visualize: Option<String>,
}

impl ProblemSummary {
    /// Construct a problem list row. `id` and `description` are trimmed.
    ///
    /// # Errors
    ///
    /// Returns [`ProblemSummaryError`] when `id` or `description` is empty after trim.
    pub fn new(
        id: &str,
        description: &str,
        visualize: Option<&str>,
    ) -> Result<Self, ProblemSummaryError> {
        let id = id.trim();
        let description = description.trim();
        if id.is_empty() {
            return Err(ProblemSummaryError::Id);
        }
        if description.is_empty() {
            return Err(ProblemSummaryError::Description);
        }
        let visualize = visualize
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        Ok(Self {
            id: id.to_owned(),
            description: description.to_owned(),
            visualize,
        })
    }
}

/// Why [`ProblemSummary::new`] failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProblemSummaryError {
    /// Empty id.
    Id,
    /// Empty description.
    Description,
}

/// Apply body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplyBody {
    /// Display name.
    pub display_name: DisplayName,
}

impl ApplyBody {
    /// Parse `{ display_name }`.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayNameError`] when the name fails [`DisplayName::new`].
    pub fn new(display_name: &str) -> Result<Self, DisplayNameError> {
        Ok(Self {
            display_name: DisplayName::new(display_name)?,
        })
    }
}

/// Apply response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplyResponse {
    /// Request id (wait URL only).
    pub request_id: RequestId,
    /// Wait token.
    pub wait_token: crate::id::WaitToken,
    /// Unix seconds when the pending row expires.
    pub expires_at: u64,
}

/// Access-request status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessStatus {
    /// Waiting for an operator.
    Pending,
    /// Approved.
    Approved,
    /// Denied.
    Denied,
    /// Pending TTL elapsed.
    Expired,
}

impl AccessStatus {
    /// Wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
        }
    }

    /// Parse a wire string.
    ///
    /// # Errors
    ///
    /// Returns [`AccessStatusError`] when `raw` is unknown.
    pub fn parse(raw: &str) -> Result<Self, AccessStatusError> {
        match raw {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            _ => Err(AccessStatusError::Unknown),
        }
    }
}

/// Unknown access status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessStatusError {
    /// Not in the v1 set.
    Unknown,
}

impl fmt::Display for AccessStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown access status")
    }
}

/// Access-request row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessRequest {
    /// Request id.
    pub request_id: RequestId,
    /// Display name.
    pub display_name: DisplayName,
    /// Client address string.
    pub client_addr: String,
    /// Created unix seconds.
    pub created_at: u64,
    /// Pending expiry unix seconds.
    pub expires_at: u64,
    /// Status.
    pub status: AccessStatus,
}

/// Decision sent to the waiter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    /// Terminal status.
    pub status: DecisionStatus,
    /// JWT when approved (wait-token principal only).
    pub jwt: Option<Jwt>,
    /// Session id when approved.
    pub session_id: Option<SessionId>,
    /// Session expiry when approved.
    pub expires_at: Option<u64>,
    /// Session WebSocket URL when approved.
    pub ws_url: Option<String>,
}

/// Terminal wait status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionStatus {
    /// Approved.
    Approved,
    /// Denied.
    Denied,
    /// Expired.
    Expired,
}

impl DecisionStatus {
    /// Wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
        }
    }

    /// Parse a wire string.
    ///
    /// # Errors
    ///
    /// Returns [`DecisionStatusError`] when `raw` is unknown.
    pub fn parse(raw: &str) -> Result<Self, DecisionStatusError> {
        match raw {
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            _ => Err(DecisionStatusError::Unknown),
        }
    }
}

/// Unknown decision status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionStatusError {
    /// Not in the v1 set.
    Unknown,
}

impl fmt::Display for DecisionStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown decision status")
    }
}

/// Approve body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApproveBody {
    /// TTL override; `None` means [`DEFAULT_TTL_SECONDS`].
    pub ttl_seconds: Option<u32>,
}

/// Why [`ApproveBody::new`] failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TtlError {
    /// Zero, or greater than [`MAX_TTL_SECONDS`].
    Range,
}

impl fmt::Display for TtlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ttl_seconds must be 1..=604800")
    }
}

impl ApproveBody {
    /// `ttl_seconds` null or `1..=MaxTtlSeconds`.
    ///
    /// # Errors
    ///
    /// Returns [`TtlError::Range`] when `ttl_seconds` is `Some(0)` or above the max.
    pub fn new(ttl_seconds: Option<u32>) -> Result<Self, TtlError> {
        if let Some(ttl) = ttl_seconds
            && (ttl == 0 || ttl > MAX_TTL_SECONDS)
        {
            return Err(TtlError::Range);
        }
        Ok(Self { ttl_seconds })
    }

    /// Effective TTL.
    #[must_use]
    pub fn effective_ttl(self) -> u32 {
        self.ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS)
    }
}

/// Login body. `Debug` omits the password.
#[derive(Clone, Eq, PartialEq)]
pub struct LoginBody {
    password: String,
}

impl LoginBody {
    /// Non-empty password.
    ///
    /// # Errors
    ///
    /// Returns [`LoginError::Empty`] when the password is empty.
    pub fn new(password: &str) -> Result<Self, LoginError> {
        if password.is_empty() {
            return Err(LoginError::Empty);
        }
        Ok(Self {
            password: password.to_owned(),
        })
    }

    /// Password bytes for argon2id verify. Callers MUST NOT log this.
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for LoginBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginBody")
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Empty login password.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoginError {
    /// Empty password.
    Empty,
}

impl fmt::Display for LoginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("password is empty")
    }
}

/// `{ enabled: bool }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnabledBody {
    /// Enabled flag.
    pub enabled: bool,
}

impl EnabledBody {
    /// Construct.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

/// Extend body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtendBody {
    /// Seconds to add, subject to max TTL from original `iat`.
    pub ttl_seconds: u32,
}

impl ExtendBody {
    /// `ttl_seconds` in `1..=MaxTtlSeconds`.
    ///
    /// # Errors
    ///
    /// Returns [`TtlError::Range`] when the value is 0 or above the max.
    pub fn new(ttl_seconds: u32) -> Result<Self, TtlError> {
        if ttl_seconds == 0 || ttl_seconds > MAX_TTL_SECONDS {
            return Err(TtlError::Range);
        }
        Ok(Self { ttl_seconds })
    }
}

/// Live session row from Docker labels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRow {
    /// Session id.
    pub session_id: SessionId,
    /// Language.
    pub language: LanguageId,
    /// Expiry unix seconds.
    pub expiry: u64,
    /// Display name.
    pub display_name: DisplayName,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{Jwt, RequestId, SessionId, WaitToken};

    const HEX: &str = "0123456789abcdef0123456789abcdef";
    const WAIT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmno-_";

    #[test]
    fn routes_include_stop_names() {
        let names: Vec<&str> = HTTP_ROUTES.iter().map(|r| r.name).collect();
        for need in ["apply", "approve", "languages", "problems", "login"] {
            assert!(names.contains(&need), "{need}");
        }
        assert!(HTTP_ROUTES.iter().any(|r| r.listener == Listener::Public));
        assert!(HTTP_ROUTES.iter().any(|r| r.listener == Listener::Admin));
        assert_eq!(HTTP_ROUTES[0].method, "POST");
    }

    #[test]
    fn error_codes_round_trip() {
        for code in [
            ErrorCode::Full,
            ErrorCode::UnavailableLanguage,
            ErrorCode::LanguageMismatch,
            ErrorCode::UnknownProblem,
            ErrorCode::BadToken,
            ErrorCode::Denied,
            ErrorCode::Expired,
            ErrorCode::StartFailed,
            ErrorCode::BadRequest,
        ] {
            assert_eq!(ErrorCode::parse(code.as_str()).expect("code"), code);
        }
        assert_eq!(ErrorCode::parse("nope"), Err(ErrorCodeError::Unknown));
        assert_eq!(ErrorCodeError::Unknown.to_string(), "unknown error code");
        let body = ErrorBody::new(ErrorCode::Full, "  ");
        assert_eq!(body.message, "full");
        let body = ErrorBody::new(ErrorCode::Denied, " no ");
        assert_eq!(body.message, "no");
    }

    #[test]
    fn language_ids() {
        for id in LanguageId::all() {
            assert_eq!(LanguageId::parse(id.as_str()).expect("id"), *id);
        }
        assert_eq!(LanguageId::parse("c"), Err(LanguageIdError::Unknown));
        assert_eq!(LanguageIdError::Unknown.to_string(), "unknown language id");
        let lang = Language::new(
            LanguageId::Python,
            "img@sha256:aa",
            "256Mi",
            1.0,
            128,
            "64Mi",
            true,
        )
        .expect("lang");
        assert!(lang.enabled);
        assert_eq!(
            Language::new(LanguageId::Go, "i", "m", 0.0, 1, "t", false),
            Err(LanguageError::Cpus)
        );
        assert_eq!(
            Language::new(LanguageId::Go, "i", "m", f64::NAN, 1, "t", false),
            Err(LanguageError::Cpus)
        );
        assert_eq!(
            Language::new(LanguageId::Go, "", "m", 1.0, 1, "t", false),
            Err(LanguageError::Empty)
        );
        assert_eq!(
            Language::new(LanguageId::Go, "i", "", 1.0, 1, "t", false),
            Err(LanguageError::Empty)
        );
        assert_eq!(
            Language::new(LanguageId::Go, "i", "m", 1.0, 1, "", false),
            Err(LanguageError::Empty)
        );
        assert_eq!(
            Language::new(LanguageId::Go, "i", "m", f64::NEG_INFINITY, 1, "t", false),
            Err(LanguageError::Cpus)
        );
    }

    #[test]
    fn problem_summary_and_apply() {
        let p = ProblemSummary::new("int-sum", " sum ", Some("  ")).expect("p");
        assert_eq!(p.id, "int-sum");
        assert_eq!(p.description, "sum");
        assert_eq!(p.visualize, None);
        let p = ProblemSummary::new("int-sum", "sum", Some("grid")).expect("p");
        assert_eq!(p.visualize.as_deref(), Some("grid"));
        assert_eq!(
            ProblemSummary::new("  ", "d", None),
            Err(ProblemSummaryError::Id)
        );
        assert_eq!(
            ProblemSummary::new("id", "  ", None),
            Err(ProblemSummaryError::Description)
        );
        let apply = ApplyBody::new("  Ada ").expect("apply");
        assert_eq!(apply.display_name.as_str(), "Ada");
        assert!(ApplyBody::new("").is_err());
        let _ = ApplyResponse {
            request_id: RequestId::new(HEX).expect("id"),
            wait_token: WaitToken::new(WAIT).expect("w"),
            expires_at: 1,
        };
    }

    #[test]
    fn access_and_decision() {
        for s in [
            AccessStatus::Pending,
            AccessStatus::Approved,
            AccessStatus::Denied,
            AccessStatus::Expired,
        ] {
            assert_eq!(AccessStatus::parse(s.as_str()).expect("s"), s);
        }
        assert_eq!(AccessStatus::parse("x"), Err(AccessStatusError::Unknown));
        assert!(AccessStatusError::Unknown.to_string().contains("status"));
        for s in [
            DecisionStatus::Approved,
            DecisionStatus::Denied,
            DecisionStatus::Expired,
        ] {
            assert_eq!(DecisionStatus::parse(s.as_str()).expect("s"), s);
        }
        assert_eq!(
            DecisionStatus::parse("x"),
            Err(DecisionStatusError::Unknown)
        );
        assert!(
            DecisionStatusError::Unknown
                .to_string()
                .contains("decision")
        );
        let row = AccessRequest {
            request_id: RequestId::new(HEX).expect("id"),
            display_name: DisplayName::new("Ada").expect("n"),
            client_addr: "1.2.3.4".into(),
            created_at: 1,
            expires_at: 2,
            status: AccessStatus::Pending,
        };
        assert_eq!(row.client_addr, "1.2.3.4");
        let d = Decision {
            status: DecisionStatus::Approved,
            jwt: Some(Jwt::new("a.b.c").expect("j")),
            session_id: Some(SessionId::new(HEX).expect("s")),
            expires_at: Some(3),
            ws_url: Some("/v1/sessions/x/channel".into()),
        };
        assert!(d.jwt.is_some());
    }

    #[test]
    fn ttl_login_extend_enabled() {
        assert_eq!(
            ApproveBody::new(None).expect("d").effective_ttl(),
            DEFAULT_TTL_SECONDS
        );
        assert_eq!(ApproveBody::new(Some(60)).expect("t").ttl_seconds, Some(60));
        assert_eq!(ApproveBody::new(Some(0)), Err(TtlError::Range));
        assert_eq!(
            ApproveBody::new(Some(MAX_TTL_SECONDS + 1)),
            Err(TtlError::Range)
        );
        assert_eq!(
            ApproveBody::new(Some(MAX_TTL_SECONDS))
                .expect("max")
                .effective_ttl(),
            MAX_TTL_SECONDS
        );
        assert!(TtlError::Range.to_string().contains("604800"));
        let pw: String = (b'a'..=b'h').map(char::from).collect();
        let login = LoginBody::new(&pw).expect("pw");
        assert_eq!(login.password(), pw);
        let dbg = format!("{login:?}");
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains(&pw));
        assert_eq!(LoginBody::new(""), Err(LoginError::Empty));
        assert_eq!(LoginError::Empty.to_string(), "password is empty");
        assert!(EnabledBody::new(true).enabled);
        assert!(!EnabledBody::new(false).enabled);
        assert_eq!(ExtendBody::new(10).expect("e").ttl_seconds, 10);
        assert_eq!(ExtendBody::new(0), Err(TtlError::Range));
        assert_eq!(ExtendBody::new(MAX_TTL_SECONDS + 1), Err(TtlError::Range));
        let sess = SessionRow {
            session_id: SessionId::new(HEX).expect("s"),
            language: LanguageId::Rust,
            expiry: 9,
            display_name: DisplayName::new("Ada").expect("n"),
        };
        assert_eq!(sess.expiry, 9);
        assert_eq!(PENDING_TTL_SECONDS, 900);
    }
}
