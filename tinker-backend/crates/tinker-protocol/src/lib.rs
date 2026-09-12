//! Mixtrapi HTTP JSON types and `tinker-http.js` codegen input.
//!
//! Field names on these types are the JSON names on the public and admin listeners.
//! [`javascript_module`] is the text of `generated/tinker-http.js`.

mod http;
mod id;
mod js;

pub use http::{
    AccessRequest, AccessStatus, AccessStatusError, ApplyBody, ApplyResponse, ApproveBody,
    DEFAULT_TTL_SECONDS, Decision, DecisionStatus, DecisionStatusError, EnabledBody, ErrorBody,
    ErrorCode, ErrorCodeError, ExtendBody, HTTP_ROUTES, HttpRoute, Language, LanguageError,
    LanguageId, LanguageIdError, Listener, LoginBody, LoginError, MAX_TTL_SECONDS,
    PENDING_TTL_SECONDS, ProblemSummary, ProblemSummaryError, SessionRow, TtlError,
};
pub use id::{
    DisplayName, DisplayNameError, HexIdError, Jwt, JwtError, RequestId, SessionId, WaitToken,
    WaitTokenError,
};
pub use js::javascript_module;

/// Protocol id on the session WebSocket and in Hello.
pub const PROTOCOL_ID: &str = "mixtrapi/1";

/// Directory name for generated HTTP helpers beside the host workspace.
pub const GENERATED_DIR: &str = "generated";

/// File name of the HTTP JavaScript module.
pub const HTTP_JS_FILE: &str = "tinker-http.js";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_id_is_mixtrapi_1() {
        assert_eq!(PROTOCOL_ID, "mixtrapi/1");
        assert_eq!(GENERATED_DIR, "generated");
        assert_eq!(HTTP_JS_FILE, "tinker-http.js");
    }
}
