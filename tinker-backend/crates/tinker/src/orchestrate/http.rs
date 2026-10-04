//! Public and admin HTTP listeners.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use tinker_protocol::{
    AccessRequest, ApplyBody, ApplyResponse, ApproveBody, Decision, ErrorBody, ErrorCode, LoginBody,
};

use super::admission::{self, Admission, AdmitError};
use super::config::Config;
use super::jwt;

/// Running listeners.
pub struct Running {
    /// Public bind including ephemeral port.
    pub public: SocketAddr,
    /// Admin bind.
    pub admin: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
    join: tokio::task::JoinHandle<()>,
}

impl Running {
    /// Stop both listeners.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.join.await;
    }
}

/// Bind public and admin sockets.
///
/// # Errors
///
/// Returns a message when either bind fails.
pub async fn serve(cfg: Config, admission: Arc<Admission>) -> Result<Running, String> {
    let public_listener = tokio::net::TcpListener::bind(cfg.public_listen)
        .await
        .map_err(super::display_err)?;
    let admin_listener = tokio::net::TcpListener::bind(cfg.admin_listen)
        .await
        .map_err(super::display_err)?;
    let public = public_listener.local_addr().map_err(super::display_err)?;
    let admin = admin_listener.local_addr().map_err(super::display_err)?;
    let (tx, rx) = tokio::sync::watch::channel(false);
    let pub_state = admission.clone();
    let adm_state = admission;
    let mut rx_pub = rx.clone();
    let mut rx_adm = rx;
    let join = tokio::spawn(async move {
        let public_app =
            public_router(pub_state).into_make_service_with_connect_info::<SocketAddr>();
        let admin_app = admin_router(adm_state).into_make_service_with_connect_info::<SocketAddr>();
        let pub_srv = axum::serve(public_listener, public_app).with_graceful_shutdown(async move {
            let _ = rx_pub.wait_for(|s| *s).await;
        });
        let adm_srv = axum::serve(admin_listener, admin_app).with_graceful_shutdown(async move {
            let _ = rx_adm.wait_for(|s| *s).await;
        });
        let _ = tokio::join!(pub_srv, adm_srv);
    });
    Ok(Running {
        public,
        admin,
        shutdown: tx,
        join,
    })
}

fn public_router(state: Arc<Admission>) -> Router {
    Router::new()
        .route("/", get(|| async { "Tinker\n" }))
        .route("/v1/access-requests", post(apply))
        .route("/v1/access-requests/{request_id}", get(poll))
        .route("/v1/access-requests/{request_id}/wait", get(wait))
        .route("/v1/languages", get(|| async { json_ok("[]") }))
        .route("/v1/problems", get(problems_public))
        .route("/v1/sessions/resume", post(not_implemented))
        .route(
            "/v1/sessions/{session_id}/saves",
            get(not_implemented).post(not_implemented),
        )
        .route(
            "/v1/sessions/{session_id}/saves/{label}/restore",
            post(not_implemented),
        )
        .route("/v1/sessions/{session_id}/channel", get(not_implemented))
        .with_state(state)
}

fn admin_router(state: Arc<Admission>) -> Router {
    Router::new()
        .route("/", get(|| async { "Tinker admin\n" }))
        .route("/v1/login", post(login))
        .route("/v1/logout", post(logout))
        .route("/v1/access-requests", get(list_requests))
        .route("/v1/access-requests/{request_id}/approve", post(approve))
        .route("/v1/access-requests/{request_id}/deny", post(deny))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{session_id}/revoke", post(revoke))
        .route(
            "/v1/sessions/{session_id}/extend",
            post(admin_not_implemented),
        )
        .route("/v1/events", get(admin_not_implemented))
        .route("/v1/compute-nodes", get(admin_not_implemented))
        .route("/v1/languages", get(admin_guard_empty))
        .route("/v1/languages/{id}/enabled", post(admin_not_implemented))
        .route("/v1/problems", get(admin_guard_empty))
        .with_state(state)
}

async fn apply(State(st): State<Arc<Admission>>, req: Request<Body>) -> Response {
    let addr = client_addr(&req);
    let body = match read_json(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let apply = match body.get("display_name").and_then(|v| v.as_str()) {
        Some(s) => match ApplyBody::new(s) {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::BadRequest,
                    "display name must be 1 to 64 characters",
                );
            }
        },
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                ErrorCode::BadRequest,
                "display_name is required",
            );
        }
    };
    match st.apply(apply.display_name, &addr) {
        Ok(r) => json_status(StatusCode::OK, &apply_json(&r)),
        Err(e) => admit_err(e),
    }
}

async fn poll(
    State(st): State<Arc<Admission>>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return err(
            StatusCode::UNAUTHORIZED,
            ErrorCode::BadToken,
            "wait token required",
        );
    };
    match st.poll(&request_id, token) {
        Ok((row, decision)) => json_ok(&access_json(&row, decision.as_ref(), true)),
        Err(e) => admit_err(e),
    }
}

async fn wait(
    State(st): State<Arc<Admission>>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return err(
            StatusCode::UNAUTHORIZED,
            ErrorCode::BadToken,
            "wait token required",
        );
    };
    let token = token.to_owned();
    ws.on_upgrade(move |socket| wait_socket(st, request_id, token, socket))
}

async fn wait_socket(st: Arc<Admission>, request_id: String, token: String, mut socket: WebSocket) {
    loop {
        match st.poll(&request_id, &token) {
            Ok((_, Some(d))) => {
                let _ = socket.send(Message::text(decision_json(&d, true))).await;
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
            Ok(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
            Err(_) => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
        }
    }
}

async fn problems_public(headers: HeaderMap, State(st): State<Arc<Admission>>) -> Response {
    let Some(tok) = bearer(&headers) else {
        return err(
            StatusCode::UNAUTHORIZED,
            ErrorCode::BadToken,
            "bearer JWT required",
        );
    };
    match jwt::verify(&st.config().jwt_hs256_secret, tok, st.now()) {
        Ok(c) if !st.is_revoked(c.session_id.as_str(), st.now()) => json_ok("[]"),
        _ => err(
            StatusCode::UNAUTHORIZED,
            ErrorCode::BadToken,
            "JWT rejected",
        ),
    }
}

async fn login(State(st): State<Arc<Admission>>, req: Request<Body>) -> Response {
    let https = is_https(&req);
    let addr = client_addr(&req);
    let body = match read_json(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let pw = match body.get("password").and_then(|v| v.as_str()) {
        Some(p) => match LoginBody::new(p) {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::BadRequest,
                    "password is empty",
                );
            }
        },
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                ErrorCode::BadRequest,
                "password is required",
            );
        }
    };
    match st.login(pw.password(), &addr, https) {
        Ok(token) => {
            let cookie = admission::set_cookie(&token, https);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::SET_COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("resp")
        }
        Err(e) => admit_err(e),
    }
}

async fn logout(
    State(st): State<Arc<Admission>>,
    headers: HeaderMap,
    req: Request<Body>,
) -> Response {
    let https = is_https(&req);
    st.logout(admission::cookie_value(
        headers.get(header::COOKIE).and_then(|v| v.to_str().ok()),
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, admission::clear_cookie(https))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("resp")
}

async fn list_requests(State(st): State<Arc<Admission>>, headers: HeaderMap) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    let rows: Vec<_> = st
        .list_requests()
        .iter()
        .map(|r| access_json(r, None, false))
        .collect();
    json_ok(&format!("[{}]", rows.join(",")))
}

async fn approve(
    State(st): State<Arc<Admission>>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    req: Request<Body>,
) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    let body = match read_json(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let ttl = match body.get("ttl_seconds") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Some(n),
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::BadRequest,
                    "ttl_seconds is invalid",
                );
            }
        },
    };
    let persist = body
        .get("persist")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let retention = match body.get("retention_seconds") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Some(n),
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::BadRequest,
                    "retention_seconds is invalid",
                );
            }
        },
    };
    let approve = match ApproveBody::full(ttl, persist, retention) {
        Ok(b) => b,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                ErrorCode::BadRequest,
                "ttl_seconds must be 1..=604800",
            );
        }
    };
    match st.approve(&request_id, approve) {
        Ok(d) => json_ok(&decision_json(&d, false)),
        Err(e) => admit_err(e),
    }
}

async fn deny(
    State(st): State<Arc<Admission>>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    match st.deny(&request_id) {
        Ok(d) => json_ok(&decision_json(&d, false)),
        Err(e) => admit_err(e),
    }
}

async fn list_sessions(State(st): State<Arc<Admission>>, headers: HeaderMap) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    json_ok("[]")
}

async fn revoke(
    State(st): State<Arc<Admission>>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    match st.revoke(&session_id) {
        Ok(()) => json_ok("{}"),
        Err(e) => admit_err(e),
    }
}

async fn admin_guard_empty(State(st): State<Arc<Admission>>, headers: HeaderMap) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    json_ok("[]")
}

async fn not_implemented() -> Response {
    err(
        StatusCode::NOT_IMPLEMENTED,
        ErrorCode::BadRequest,
        "not implemented",
    )
}

async fn admin_not_implemented(State(st): State<Arc<Admission>>, headers: HeaderMap) -> Response {
    if !st.admin_ok(cookie(&headers)) {
        return admit_err(AdmitError::Unauthorized);
    }
    not_implemented().await
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    admission::cookie_value(headers.get(header::COOKIE).and_then(|v| v.to_str().ok()))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

fn client_addr(req: &Request<Body>) -> String {
    req.extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn is_https(req: &Request<Body>) -> bool {
    req.uri().scheme_str() == Some("https")
        || req
            .headers()
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            == Some("https")
}

#[allow(clippy::result_large_err)]
async fn read_json(req: Request<Body>) -> Result<serde_json::Value, Response> {
    let bytes = axum::body::to_bytes(req.into_body(), 64 * 1024)
        .await
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                ErrorCode::BadRequest,
                "body is too large or incomplete",
            )
        })?;
    if bytes.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            "JSON body is invalid",
        )
    })
}

fn admit_err(e: AdmitError) -> Response {
    match e {
        AdmitError::RateLimited => err(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::BadRequest,
            "rate limit exceeded",
        ),
        AdmitError::Unauthorized => err(
            StatusCode::UNAUTHORIZED,
            ErrorCode::BadToken,
            "sign in with the admin password",
        ),
        AdmitError::Code(code, msg) => {
            let status = match code {
                ErrorCode::BadToken => StatusCode::UNAUTHORIZED,
                ErrorCode::Expired | ErrorCode::Denied => StatusCode::OK,
                _ => StatusCode::BAD_REQUEST,
            };
            err(status, code, &msg)
        }
    }
}

fn err(status: StatusCode, code: ErrorCode, message: &str) -> Response {
    let body = ErrorBody::new(code, message);
    json_status(
        status,
        &format!(
            "{{\"error\":\"{}\",\"message\":{}}}",
            body.error.as_str(),
            serde_json::Value::String(body.message)
        ),
    )
}

fn json_ok(body: &str) -> Response {
    json_status(StatusCode::OK, body)
}

fn json_status(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("resp")
}

fn apply_json(r: &ApplyResponse) -> String {
    format!(
        "{{\"request_id\":\"{}\",\"wait_token\":\"{}\",\"expires_at\":{}}}",
        r.request_id, r.wait_token, r.expires_at
    )
}

fn access_json(row: &AccessRequest, decision: Option<&Decision>, include_secrets: bool) -> String {
    let mut s = format!(
        "{{\"request_id\":\"{}\",\"display_name\":{},\"created_at\":{},\"expires_at\":{},\"status\":\"{}\"",
        row.request_id,
        serde_json::Value::String(row.display_name.as_str().to_owned()),
        row.created_at,
        row.expires_at,
        row.status.as_str()
    );
    if let Some(d) = decision {
        s.push_str(",\"decision\":");
        s.push_str(&decision_json(d, include_secrets));
    }
    s.push('}');
    s
}

fn decision_json(d: &Decision, include_secrets: bool) -> String {
    let jwt = if include_secrets {
        opt_str(d.jwt.as_ref().map(tinker_protocol::Jwt::as_str))
    } else {
        "null".into()
    };
    let resume = if include_secrets {
        opt_str(
            d.resume_token
                .as_ref()
                .map(tinker_protocol::WaitToken::as_str),
        )
    } else {
        "null".into()
    };
    format!(
        "{{\"status\":\"{}\",\"jwt\":{},\"session_id\":{},\"user_id\":{},\"workspace_id\":{},\"resume_token\":{},\"expires_at\":{},\"ws_url\":{}}}",
        d.status.as_str(),
        jwt,
        opt_str(
            d.session_id
                .as_ref()
                .map(tinker_protocol::SessionId::as_str)
        ),
        opt_str(d.user_id.as_ref().map(tinker_protocol::UserId::as_str)),
        opt_str(
            d.workspace_id
                .as_ref()
                .map(tinker_protocol::WorkspaceId::as_str)
        ),
        resume,
        d.expires_at
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".into()),
        opt_str(d.ws_url.as_deref())
    )
}

fn opt_str(s: Option<&str>) -> String {
    match s {
        Some(v) => serde_json::Value::String(v.to_owned()).to_string(),
        None => "null".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrate::admission::{Clock, Entropy};
    use crate::orchestrate::password::hash_password;
    use futures_util::StreamExt;
    use std::sync::{Arc as StdArc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tinker_protocol::{
        DecisionStatus, DisplayName, Jwt, RequestId, SessionId, UserId, WaitToken, WorkspaceId,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tower::ServiceExt;

    struct Fixed(Mutex<u64>);
    impl Clock for Fixed {
        fn unix_seconds(&self) -> u64 {
            *self.0.lock().expect("c")
        }
    }
    struct Seq(Mutex<u64>);
    impl Entropy for Seq {
        fn fill(&self, buf: &mut [u8]) {
            let mut n = self.0.lock().expect("e");
            *n = n.wrapping_add(1);
            let bytes = n.to_le_bytes();
            for (i, b) in buf.iter_mut().enumerate() {
                *b = bytes[i % 8].wrapping_add(i as u8);
            }
        }
    }

    fn scratch() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("t")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tinker-http-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("d");
        dir
    }

    fn cfg(hash: String, dir: &std::path::Path) -> Config {
        Config {
            public_listen: "127.0.0.1:0".parse().expect("p"),
            admin_listen: "127.0.0.1:0".parse().expect("a"),
            admin_password_hash: hash,
            jwt_hs256_secret: b"http-secret".to_vec(),
            default_ttl_seconds: 3600,
            max_ttl_seconds: 604_800,
            pending_ttl_seconds: 900,
            revoke_deny_file: dir.join("deny"),
        }
    }

    async fn raw_http(
        host: SocketAddr,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> (u16, String, String) {
        let mut s = tokio::net::TcpStream::connect(host).await.expect("connect");
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        if let Some(b) = body {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).await.expect("w");
        if let Some(b) = body {
            s.write_all(b).await.expect("wb");
        }
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.expect("r");
        let text = String::from_utf8_lossy(&buf).into_owned();
        let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let code = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        (code, head.to_owned(), rest.to_owned())
    }

    fn cookie_pair(head: &str) -> String {
        head.lines()
            .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
            .and_then(|l| l.split_once(':').map(|x| x.1))
            .map(str::trim)
            .and_then(|c| c.split(';').next())
            .map(str::trim)
            .expect("cookie")
            .to_owned()
    }

    fn body_json(body: &str) -> serde_json::Value {
        let json = body.trim_end_matches(|c: char| c.is_ascii_whitespace() || c == '0');
        let json = json.strip_prefix("1\r\n").unwrap_or(json);
        serde_json::from_str(json.trim()).unwrap_or_else(|_| {
            let start = body.find('{').or_else(|| body.find('[')).unwrap_or(0);
            let end = body.rfind(['}', ']']).map(|i| i + 1).unwrap_or(body.len());
            serde_json::from_str(&body[start..end]).expect("json")
        })
    }

    #[test]
    fn json_helpers_and_admit_err() {
        let hex = "0123456789abcdef0123456789abcdef";
        let wait = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmno-_";
        let d = Decision {
            status: DecisionStatus::Denied,
            jwt: None,
            session_id: None,
            user_id: None,
            workspace_id: None,
            resume_token: None,
            expires_at: None,
            ws_url: None,
        };
        assert!(decision_json(&d, true).contains("\"jwt\":null"));
        let approved = Decision {
            status: DecisionStatus::Approved,
            jwt: Some(Jwt::new("aaa.bbb.ccc").expect("j")),
            session_id: Some(SessionId::new(hex).expect("s")),
            user_id: Some(UserId::new(hex).expect("u")),
            workspace_id: Some(WorkspaceId::new(hex).expect("w")),
            resume_token: Some(WaitToken::new(wait).expect("t")),
            expires_at: Some(9),
            ws_url: Some("/v1/sessions/x/channel".into()),
        };
        let open = decision_json(&approved, true);
        assert!(open.contains("aaa.bbb.ccc"));
        assert!(open.contains(wait));
        let hidden = decision_json(&approved, false);
        assert!(hidden.contains("\"jwt\":null"));
        assert!(hidden.contains("\"resume_token\":null"));
        let row = AccessRequest {
            request_id: RequestId::new(hex).expect("r"),
            display_name: DisplayName::new("Ada").expect("n"),
            client_addr: "1.1.1.1".into(),
            created_at: 1,
            expires_at: 2,
            status: tinker_protocol::AccessStatus::Pending,
        };
        assert!(access_json(&row, Some(&d), true).contains("decision"));
        let r = admit_err(AdmitError::Code(ErrorCode::Expired, "x".into()));
        assert_eq!(r.status(), StatusCode::OK);
        let r = admit_err(AdmitError::Code(ErrorCode::Denied, "x".into()));
        assert_eq!(r.status(), StatusCode::OK);
        let r = admit_err(AdmitError::Code(ErrorCode::StartFailed, "x".into()));
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let r = admit_err(AdmitError::RateLimited);
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(opt_str(None), "null");
        assert_eq!(opt_str(Some("a")), "\"a\"");
        assert_eq!(body_json("[true]"), serde_json::json!([true]));
        assert_eq!(body_json("chunk\n[false]\n"), serde_json::json!([false]));
    }

    #[tokio::test]
    async fn bind_failures() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let mut c = cfg(hash.clone(), &dir);
        c.public_listen = "255.255.255.255:1".parse().expect("p");
        let adm = StdArc::new(
            Admission::new(
                c.clone(),
                Box::new(Fixed(Mutex::new(1))),
                Box::new(Seq(Mutex::new(1))),
            )
            .expect("a"),
        );
        assert!(serve(c, adm.clone()).await.is_err());
        let mut c = cfg(hash, &dir);
        c.admin_listen = "255.255.255.255:1".parse().expect("a");
        assert!(serve(c, adm).await.is_err());
    }

    #[tokio::test]
    async fn oneshot_unknown_addr_and_https_cookie() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let c = cfg(hash, &dir);
        let adm = StdArc::new(
            Admission::new(
                c,
                Box::new(Fixed(Mutex::new(1_700_000_000))),
                Box::new(Seq(Mutex::new(3))),
            )
            .expect("a"),
        );
        let app = public_router(adm.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/access-requests")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{\"display_name\":\"Ada\"}"))
                    .expect("req"),
            )
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);

        let admin = admin_router(adm);
        let res = admin
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-forwarded-proto", "https")
                    .body(Body::from("{\"password\":\"pw\"}"))
                    .expect("req"),
            )
            .await
            .expect("login");
        let set = res
            .headers()
            .get(header::SET_COOKIE)
            .expect("cookie")
            .to_str()
            .expect("utf8");
        assert!(set.contains("Secure"));
    }

    #[tokio::test]
    async fn apply_login_approve_wait() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let c = cfg(hash, &dir);
        let adm = StdArc::new(
            Admission::new(
                c.clone(),
                Box::new(Fixed(Mutex::new(1_700_000_000))),
                Box::new(Seq(Mutex::new(9))),
            )
            .expect("adm"),
        );
        let running = serve(c, adm.clone()).await.expect("bind");
        let public = running.public;
        let admin = running.admin;

        let (code, _, body) = raw_http(public, "GET", "/", &[], None).await;
        assert_eq!(code, 200);
        assert!(body.contains("Tinker"));
        let (code, _, _) = raw_http(admin, "GET", "/", &[], None).await;
        assert_eq!(code, 200);

        let (code, _, body) = raw_http(
            public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(br#"{"display_name":"Ada"}"#),
        )
        .await;
        assert_eq!(code, 200, "{body}");
        let v = body_json(&body);
        let rid = v["request_id"].as_str().expect("id").to_owned();
        let wait = v["wait_token"].as_str().expect("w").to_owned();

        let (code, _, _) = raw_http(
            public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(br#"{"display_name":""}"#),
        )
        .await;
        assert_eq!(code, 400);
        let (code, _, body) =
            raw_http(public, "POST", "/v1/access-requests", &[], Some(b"{}")).await;
        assert_eq!(code, 400);
        assert!(body.contains("display_name"));
        let (code, _, _) = raw_http(public, "POST", "/v1/access-requests", &[], Some(b"{")).await;
        assert_eq!(code, 400);
        let long = format!("{{\"display_name\":\"{}\"}}", "é".repeat(65));
        let (code, _, _) = raw_http(
            public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(long.as_bytes()),
        )
        .await;
        assert_eq!(code, 400);

        let (code, _, _) = raw_http(
            public,
            "GET",
            &format!("/v1/access-requests/{rid}"),
            &[],
            None,
        )
        .await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(
            public,
            "GET",
            "/v1/problems",
            &[("Authorization", "Bearer nope")],
            None,
        )
        .await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(public, "GET", "/v1/problems", &[], None).await;
        assert_eq!(code, 401);
        let (code, _, body) = raw_http(public, "GET", "/v1/languages", &[], None).await;
        assert_eq!(code, 200);
        assert!(body.contains('['));

        let (code, _, _) = raw_http(admin, "GET", "/v1/access-requests", &[], None).await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(admin, "POST", "/v1/login", &[], Some(b"{}")).await;
        assert_eq!(code, 400);
        let (code, _, _) =
            raw_http(admin, "POST", "/v1/login", &[], Some(br#"{"password":""}"#)).await;
        assert_eq!(code, 400);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            "/v1/login",
            &[],
            Some(br#"{"password":"wrong"}"#),
        )
        .await;
        assert_eq!(code, 401);

        let (code, head, _) = raw_http(
            admin,
            "POST",
            "/v1/login",
            &[("X-Forwarded-Proto", "https")],
            Some(br#"{"password":"pw"}"#),
        )
        .await;
        assert_eq!(code, 200);
        assert!(head.to_ascii_lowercase().contains("secure"));
        let cookie = cookie_pair(&head);

        let (code, _, body) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(br#"{"ttl_seconds":"no"}"#),
        )
        .await;
        assert_eq!(code, 400, "{body}");
        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(br#"{"retention_seconds":"no"}"#),
        )
        .await;
        assert_eq!(code, 400);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(br#"{"ttl_seconds":0}"#),
        )
        .await;
        assert_eq!(code, 400);

        let path = format!("/v1/access-requests/{rid}/wait");
        let wait_token = wait.clone();
        let public_ws = public;
        let waiter = tokio::spawn(async move {
            let mut req = format!("ws://{public_ws}{path}")
                .into_client_request()
                .expect("ws req");
            req.headers_mut().insert(
                "Authorization",
                format!("Bearer {wait_token}").parse().expect("h"),
            );
            let (mut ws, _) = tokio_tungstenite::connect_async(req).await.expect("ws");
            ws.next()
                .await
                .expect("frame")
                .expect("ok")
                .into_text()
                .expect("text")
                .to_string()
        });
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;

        let (code, _, body) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(br#"{"ttl_seconds":60,"persist":true}"#),
        )
        .await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains("user_id"), "{body}");
        assert!(body.contains("workspace_id"), "{body}");
        assert!(body.contains("\"jwt\":null"), "{body}");

        let text = waiter.await.expect("join");
        assert!(text.contains("user_id"), "{text}");
        assert!(text.contains("\"jwt\":"), "{text}");

        let auth = format!("Bearer {wait}");
        let (code, _, body) = raw_http(
            public,
            "GET",
            &format!("/v1/access-requests/{rid}"),
            &[("Authorization", &auth)],
            None,
        )
        .await;
        assert_eq!(code, 200, "{body}");
        let polled = body_json(&body);
        let jwt = polled["decision"]["jwt"].as_str().expect("jwt").to_owned();
        let sid = polled["decision"]["session_id"]
            .as_str()
            .expect("sid")
            .to_owned();

        let bearer_jwt = format!("Bearer {jwt}");
        let (code, _, body) = raw_http(
            public,
            "GET",
            "/v1/problems",
            &[("Authorization", &bearer_jwt)],
            None,
        )
        .await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains('['));

        let (code, _, body) = raw_http(
            admin,
            "GET",
            "/v1/access-requests",
            &[("Cookie", &cookie)],
            None,
        )
        .await;
        assert_eq!(code, 200);
        assert!(body.contains("Ada"), "{body}");
        for path in [
            "/v1/sessions",
            "/v1/languages",
            "/v1/problems",
            "/v1/events",
            "/v1/compute-nodes",
        ] {
            let (code, _, _) = raw_http(admin, "GET", path, &[("Cookie", &cookie)], None).await;
            assert!(code == 200 || code == 501, "{path} {code}");
        }
        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/sessions/{sid}/revoke"),
            &[("Cookie", &cookie)],
            None,
        )
        .await;
        assert_eq!(code, 200);
        let (code, _, _) = raw_http(
            public,
            "GET",
            "/v1/problems",
            &[("Authorization", &bearer_jwt)],
            None,
        )
        .await;
        assert_eq!(code, 401);

        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/sessions/{sid}/extend"),
            &[("Cookie", &cookie)],
            Some(br#"{"ttl_seconds":1}"#),
        )
        .await;
        assert_eq!(code, 501);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            "/v1/languages/python/enabled",
            &[("Cookie", &cookie)],
            Some(br#"{"enabled":true}"#),
        )
        .await;
        assert_eq!(code, 501);
        let (code, _, _) = raw_http(public, "POST", "/v1/sessions/resume", &[], Some(b"{}")).await;
        assert_eq!(code, 501);

        let (code, _, body) = raw_http(
            public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(br#"{"display_name":"Bo"}"#),
        )
        .await;
        assert_eq!(code, 200, "{body}");
        let rid2 = body_json(&body)["request_id"]
            .as_str()
            .expect("id")
            .to_owned();
        let (code, _, body) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid2}/deny"),
            &[("Cookie", &cookie)],
            None,
        )
        .await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains("denied"));

        let (code, _, _) =
            raw_http(admin, "POST", "/v1/logout", &[("Cookie", &cookie)], None).await;
        assert_eq!(code, 200);

        let unknown = "deadbeefdeadbeefdeadbeefdeadbeef";
        let mut req = format!("ws://{public}/v1/access-requests/{unknown}/wait")
            .into_client_request()
            .expect("ws");
        req.headers_mut()
            .insert("Authorization", "Bearer abc".parse().expect("h"));
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await.expect("ws");
        let closed = ws.next().await;
        assert!(closed.is_none() || matches!(closed, Some(Ok(_))));

        running.shutdown().await;
    }

    #[tokio::test]
    async fn large_body_and_rate_limit() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let c = cfg(hash, &dir);
        let adm = StdArc::new(
            Admission::new(
                c.clone(),
                Box::new(Fixed(Mutex::new(1_700_000_000))),
                Box::new(Seq(Mutex::new(1))),
            )
            .expect("adm"),
        );
        let running = serve(c, adm).await.expect("bind");
        let big = vec![b'a'; 64 * 1024 + 8];
        let (code, _, _) = raw_http(
            running.public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(&big),
        )
        .await;
        assert_eq!(code, 400);
        for _ in 0..10 {
            let (code, _, _) = raw_http(
                running.public,
                "POST",
                "/v1/access-requests",
                &[],
                Some(br#"{"display_name":"Ada"}"#),
            )
            .await;
            assert_eq!(code, 200);
        }
        let (code, _, _) = raw_http(
            running.public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(br#"{"display_name":"Ada"}"#),
        )
        .await;
        assert_eq!(code, 429);
        running.shutdown().await;
    }

    #[tokio::test]
    async fn remaining_error_branches() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let c = cfg(hash, &dir);
        let adm = StdArc::new(
            Admission::new(
                c.clone(),
                Box::new(Fixed(Mutex::new(1_700_000_000))),
                Box::new(Seq(Mutex::new(2))),
            )
            .expect("adm"),
        );
        let running = serve(c, adm).await.expect("bind");
        let public = running.public;
        let admin = running.admin;

        let (code, _, body) = raw_http(
            public,
            "POST",
            "/v1/access-requests",
            &[],
            Some(br#"{"display_name":"Ada"}"#),
        )
        .await;
        assert_eq!(code, 200, "{body}");
        let v = body_json(&body);
        let rid = v["request_id"].as_str().expect("id").to_owned();
        let wait = v["wait_token"].as_str().expect("w").to_owned();

        let (code, _, _) = raw_http(
            public,
            "GET",
            &format!("/v1/access-requests/{rid}"),
            &[("Authorization", "Bearer nope")],
            None,
        )
        .await;
        assert_eq!(code, 401);

        let req = format!("ws://{public}/v1/access-requests/{rid}/wait")
            .into_client_request()
            .expect("ws");
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err());

        let (code, _, _) = raw_http(admin, "POST", "/v1/login", &[], Some(b"{")).await;
        assert_eq!(code, 400);
        let (code, _, _) = raw_http(admin, "POST", "/v1/login", &[], None).await;
        assert_eq!(code, 400);
        let (code, head, _) = raw_http(
            admin,
            "POST",
            "/v1/login",
            &[],
            Some(br#"{"password":"pw"}"#),
        )
        .await;
        assert_eq!(code, 200);
        let cookie = cookie_pair(&head);

        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[],
            Some(br#"{}"#),
        )
        .await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(b"{"),
        )
        .await;
        assert_eq!(code, 400);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(br#"{"ttl_seconds":60,"retention_seconds":60}"#),
        )
        .await;
        assert_eq!(code, 200);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            &format!("/v1/access-requests/{rid}/approve"),
            &[("Cookie", &cookie)],
            Some(br#"{"ttl_seconds":60}"#),
        )
        .await;
        assert_eq!(code, 400);

        let (code, _, _) = raw_http(admin, "POST", "/v1/access-requests/x/deny", &[], None).await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            "/v1/access-requests/deadbeefdeadbeefdeadbeefdeadbeef/deny",
            &[("Cookie", &cookie)],
            None,
        )
        .await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(admin, "GET", "/v1/sessions", &[], None).await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(admin, "GET", "/v1/languages", &[], None).await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(admin, "GET", "/v1/events", &[], None).await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            "/v1/sessions/deadbeefdeadbeefdeadbeefdeadbeef/revoke",
            &[],
            None,
        )
        .await;
        assert_eq!(code, 401);
        let (code, _, _) = raw_http(
            admin,
            "POST",
            "/v1/sessions/deadbeefdeadbeefdeadbeefdeadbeef/revoke",
            &[("Cookie", &cookie)],
            None,
        )
        .await;
        assert_eq!(code, 401);

        let fallback = body_json("not-json {\"a\":1}");
        assert_eq!(fallback["a"], 1);
        let _ = wait;
        running.shutdown().await;
    }
}
