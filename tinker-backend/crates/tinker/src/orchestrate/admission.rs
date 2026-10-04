//! Access requests, approve/deny, admin sessions, rate limits, revoke denylist.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tinker_protocol::{
    AccessRequest, AccessStatus, ApplyResponse, ApproveBody, Decision, DecisionStatus, DisplayName,
    ErrorCode, Jwt, RequestId, SessionId, UserId, WaitToken, WorkspaceId,
};

use super::config::Config;
use super::jwt::{self, Claims};
use super::password::verify_password;

const ADMIN_COOKIE: &str = "tinker_admin";
const ADMIN_IDLE_SECS: u64 = 8 * 3600;
const APPLY_PER_HOUR: usize = 10;
const LOGIN_PER_MINUTE: usize = 5;
const MAX_PENDING: usize = 30;

/// Unix-seconds clock.
pub trait Clock: Send + Sync {
    /// Current time.
    fn unix_seconds(&self) -> u64;
}

/// Fill random bytes.
pub trait Entropy: Send + Sync {
    /// Write random bytes into `buf`.
    fn fill(&self, buf: &mut [u8]);
}

/// Host clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_seconds(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// OS RNG.
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn fill(&self, buf: &mut [u8]) {
        getrandom::getrandom(buf).expect("entropy");
    }
}

struct RateBox {
    hits: HashMap<String, Vec<u64>>,
}

impl RateBox {
    fn new() -> Self {
        Self {
            hits: HashMap::new(),
        }
    }

    fn allow(&mut self, key: &str, now: u64, limit: usize, window: u64) -> bool {
        let e = self.hits.entry(key.to_owned()).or_default();
        e.retain(|t| now.saturating_sub(*t) < window);
        if e.len() >= limit {
            return false;
        }
        e.push(now);
        true
    }

    fn count_failures(&mut self, key: &str, now: u64, window: u64) -> usize {
        let e = self.hits.entry(key.to_owned()).or_default();
        e.retain(|t| now.saturating_sub(*t) < window);
        e.len()
    }

    fn push_failure(&mut self, key: &str, now: u64, window: u64) {
        let e = self.hits.entry(key.to_owned()).or_default();
        e.retain(|t| now.saturating_sub(*t) < window);
        e.push(now);
    }
}

struct Row {
    request: AccessRequest,
    wait_hash: [u8; 32],
    jwt: Option<Jwt>,
    user_id: Option<UserId>,
    workspace_id: Option<WorkspaceId>,
    session_id: Option<SessionId>,
    jwt_exp: Option<u64>,
}

/// In-process admission state.
pub struct Admission {
    cfg: Config,
    clock: Box<dyn Clock>,
    entropy: Box<dyn Entropy>,
    rows: Mutex<HashMap<String, Row>>,
    apply_rate: Mutex<RateBox>,
    login_fail: Mutex<RateBox>,
    admin: Mutex<HashMap<String, u64>>,
    sessions: Mutex<HashMap<String, u64>>,
    denylist: Mutex<Vec<(String, u64)>>,
    deny_path: PathBuf,
}

/// Apply/login/approve failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmitError {
    /// Machine error for the HTTP body.
    Code(ErrorCode, String),
    /// HTTP 429.
    RateLimited,
    /// HTTP 401.
    Unauthorized,
}

impl Admission {
    /// Load denylist and wrap config.
    ///
    /// # Errors
    ///
    /// Returns IO errors from creating or reading the denylist file.
    pub fn new(
        cfg: Config,
        clock: Box<dyn Clock>,
        entropy: Box<dyn Entropy>,
    ) -> Result<Self, String> {
        let deny_path = cfg.revoke_deny_file.clone();
        if let Some(parent) = deny_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(super::display_err)?;
        }
        if !deny_path.exists() {
            File::create(&deny_path).map_err(super::display_err)?;
        }
        let denylist = load_denylist(&deny_path, clock.unix_seconds());
        Ok(Self {
            cfg,
            clock,
            entropy,
            rows: Mutex::new(HashMap::new()),
            apply_rate: Mutex::new(RateBox::new()),
            login_fail: Mutex::new(RateBox::new()),
            admin: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            denylist: Mutex::new(denylist),
            deny_path,
        })
    }

    /// Configured default/max/pending TTLs.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Current unix seconds.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.clock.unix_seconds()
    }

    /// `POST /v1/access-requests`.
    ///
    /// # Errors
    ///
    /// Rate limit or pending cap.
    pub fn apply(
        &self,
        display_name: DisplayName,
        client_addr: &str,
    ) -> Result<ApplyResponse, AdmitError> {
        let now = self.now();
        {
            let mut rate = self.apply_rate.lock().expect("lock");
            if !rate.allow(client_addr, now, APPLY_PER_HOUR, 3600) {
                return Err(AdmitError::RateLimited);
            }
        }
        let mut rows = self.rows.lock().expect("lock");
        self.expire_locked(&mut rows, now);
        let pending = rows
            .values()
            .filter(|r| r.request.status == AccessStatus::Pending)
            .count();
        if pending >= MAX_PENDING {
            return Err(AdmitError::RateLimited);
        }
        let request_id = RequestId::new(&self.hex_id()).expect("id");
        let wait_raw = self.wait_token();
        let wait_token = WaitToken::new(&wait_raw).expect("wait");
        let expires_at = now.saturating_add(u64::from(self.cfg.pending_ttl_seconds));
        rows.insert(
            request_id.as_str().to_owned(),
            Row {
                request: AccessRequest {
                    request_id: request_id.clone(),
                    display_name,
                    client_addr: client_addr.to_owned(),
                    created_at: now,
                    expires_at,
                    status: AccessStatus::Pending,
                },
                wait_hash: sha256(wait_raw.as_bytes()),
                jwt: None,
                user_id: None,
                workspace_id: None,
                session_id: None,
                jwt_exp: None,
            },
        );
        Ok(ApplyResponse {
            request_id,
            wait_token,
            expires_at,
        })
    }

    /// Poll or wait-token principal decision.
    ///
    /// # Errors
    ///
    /// Unknown id or bad wait token.
    pub fn poll(
        &self,
        request_id: &str,
        wait_token: &str,
    ) -> Result<(AccessRequest, Option<Decision>), AdmitError> {
        let now = self.now();
        let mut rows = self.rows.lock().expect("lock");
        self.expire_locked(&mut rows, now);
        let row = rows.get(request_id).ok_or(AdmitError::Code(
            ErrorCode::BadToken,
            "unknown access request".into(),
        ))?;
        if !ct_hash_eq(&row.wait_hash, wait_token.as_bytes()) {
            return Err(AdmitError::Code(
                ErrorCode::BadToken,
                "wait token does not match".into(),
            ));
        }
        let req = row.request.clone();
        let decision = terminal_decision(row);
        Ok((req, decision))
    }

    /// Snapshot for the admin list.
    #[must_use]
    pub fn list_requests(&self) -> Vec<AccessRequest> {
        let now = self.now();
        let mut rows = self.rows.lock().expect("lock");
        self.expire_locked(&mut rows, now);
        let mut v: Vec<_> = rows.values().map(|r| r.request.clone()).collect();
        v.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then(a.request_id.as_str().cmp(b.request_id.as_str()))
        });
        v
    }

    /// Approve a pending request.
    ///
    /// # Errors
    ///
    /// Missing, expired, or already decided.
    pub fn approve(&self, request_id: &str, body: ApproveBody) -> Result<Decision, AdmitError> {
        let now = self.now();
        let ttl = body.effective_ttl();
        if ttl > self.cfg.max_ttl_seconds {
            return Err(AdmitError::Code(
                ErrorCode::BadRequest,
                "ttl_seconds exceeds maximum".into(),
            ));
        }
        let mut rows = self.rows.lock().expect("lock");
        self.expire_locked(&mut rows, now);
        let row = rows.get_mut(request_id).ok_or(AdmitError::Code(
            ErrorCode::BadToken,
            "unknown access request".into(),
        ))?;
        if row.request.status != AccessStatus::Pending {
            return Err(AdmitError::Code(
                ErrorCode::BadRequest,
                "access request is not pending".into(),
            ));
        }
        let session_id = SessionId::new(&self.hex_id()).expect("s");
        let user_id = UserId::new(&self.hex_id()).expect("u");
        let workspace_id = WorkspaceId::new(&self.hex_id()).expect("w");
        let exp = now.saturating_add(u64::from(ttl));
        let jwt = jwt::mint(
            &self.cfg.jwt_hs256_secret,
            &Claims {
                session_id: session_id.clone(),
                user_id: user_id.clone(),
                workspace_id: workspace_id.clone(),
                exp,
                iat: now,
            },
        );
        row.request.status = AccessStatus::Approved;
        row.jwt = Some(jwt.clone());
        row.user_id = Some(user_id.clone());
        row.workspace_id = Some(workspace_id.clone());
        row.session_id = Some(session_id.clone());
        row.jwt_exp = Some(exp);
        self.sessions
            .lock()
            .expect("lock")
            .insert(session_id.as_str().to_owned(), exp);
        Ok(Decision {
            status: DecisionStatus::Approved,
            jwt: Some(jwt),
            session_id: Some(session_id.clone()),
            user_id: Some(user_id),
            workspace_id: Some(workspace_id),
            resume_token: None,
            expires_at: Some(exp),
            ws_url: Some(format!("/v1/sessions/{session_id}/channel")),
        })
    }

    /// Deny a pending request.
    ///
    /// # Errors
    ///
    /// Missing or not pending.
    pub fn deny(&self, request_id: &str) -> Result<Decision, AdmitError> {
        let now = self.now();
        let mut rows = self.rows.lock().expect("lock");
        self.expire_locked(&mut rows, now);
        let row = rows.get_mut(request_id).ok_or(AdmitError::Code(
            ErrorCode::BadToken,
            "unknown access request".into(),
        ))?;
        if row.request.status != AccessStatus::Pending {
            return Err(AdmitError::Code(
                ErrorCode::BadRequest,
                "access request is not pending".into(),
            ));
        }
        row.request.status = AccessStatus::Denied;
        Ok(Decision {
            status: DecisionStatus::Denied,
            jwt: None,
            session_id: None,
            user_id: None,
            workspace_id: None,
            resume_token: None,
            expires_at: None,
            ws_url: None,
        })
    }

    /// Admin login. Always verifies argon2id, then applies the failure budget.
    ///
    /// # Errors
    ///
    /// Unauthorized or rate limited.
    pub fn login(
        &self,
        password: &str,
        client_addr: &str,
        https: bool,
    ) -> Result<String, AdmitError> {
        let now = self.now();
        let ok = verify_password(password, &self.cfg.admin_password_hash);
        {
            let mut fails = self.login_fail.lock().expect("lock");
            if fails.count_failures(client_addr, now, 60) >= LOGIN_PER_MINUTE {
                return Err(AdmitError::RateLimited);
            }
            if !ok {
                fails.push_failure(client_addr, now, 60);
                return Err(AdmitError::Unauthorized);
            }
        }
        let token = self.wait_token();
        self.admin.lock().expect("lock").insert(token.clone(), now);
        let _ = https;
        Ok(token)
    }

    /// Drop an admin session.
    pub fn logout(&self, cookie: Option<&str>) {
        if let Some(id) = cookie {
            self.admin.lock().expect("lock").remove(id);
        }
    }

    /// Validate admin cookie and refresh idle timer.
    #[must_use]
    pub fn admin_ok(&self, cookie: Option<&str>) -> bool {
        let Some(id) = cookie else {
            return false;
        };
        let now = self.now();
        let mut admin = self.admin.lock().expect("lock");
        match admin.get(id).copied() {
            Some(last) if now.saturating_sub(last) <= ADMIN_IDLE_SECS => {
                admin.insert(id.to_owned(), now);
                true
            }
            Some(_) => {
                admin.remove(id);
                false
            }
            None => false,
        }
    }

    /// Append `session_id` to the denylist.
    ///
    /// # Errors
    ///
    /// Unknown session or IO.
    pub fn revoke(&self, session_id: &str) -> Result<(), AdmitError> {
        let now = self.now();
        let exp = {
            let mut sessions = self.sessions.lock().expect("lock");
            sessions.remove(session_id)
        };
        let Some(exp) = exp else {
            return Err(AdmitError::Code(
                ErrorCode::BadToken,
                "unknown session".into(),
            ));
        };
        {
            let mut deny = self.denylist.lock().expect("lock");
            deny.retain(|(_, e)| *e > now);
            deny.push((session_id.to_owned(), exp));
        }
        append_deny(&self.deny_path, session_id, exp)
            .map_err(|e| AdmitError::Code(ErrorCode::StartFailed, e))?;
        Ok(())
    }

    /// Whether a session id is revoked and `exp` has not passed.
    #[must_use]
    pub fn is_revoked(&self, session_id: &str, now: u64) -> bool {
        self.denylist
            .lock()
            .expect("lock")
            .iter()
            .any(|(s, e)| s == session_id && *e > now)
    }

    /// Cookie name for Set-Cookie.
    #[must_use]
    pub fn cookie_name() -> &'static str {
        ADMIN_COOKIE
    }

    fn expire_locked(&self, rows: &mut HashMap<String, Row>, now: u64) {
        for row in rows.values_mut() {
            if row.request.status == AccessStatus::Pending && now >= row.request.expires_at {
                row.request.status = AccessStatus::Expired;
            }
        }
    }

    fn hex_id(&self) -> String {
        let mut b = [0u8; 16];
        self.entropy.fill(&mut b);
        hex_encode(&b)
    }

    fn wait_token(&self) -> String {
        let mut b = [0u8; 32];
        self.entropy.fill(&mut b);
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        URL_SAFE_NO_PAD.encode(b)
    }
}

fn terminal_decision(row: &Row) -> Option<Decision> {
    match row.request.status {
        AccessStatus::Pending => None,
        AccessStatus::Approved => Some(Decision {
            status: DecisionStatus::Approved,
            jwt: row.jwt.clone(),
            session_id: row.session_id.clone(),
            user_id: row.user_id.clone(),
            workspace_id: row.workspace_id.clone(),
            resume_token: None,
            expires_at: row.jwt_exp,
            ws_url: row
                .session_id
                .as_ref()
                .map(|s| format!("/v1/sessions/{s}/channel")),
        }),
        AccessStatus::Denied => Some(Decision {
            status: DecisionStatus::Denied,
            jwt: None,
            session_id: None,
            user_id: None,
            workspace_id: None,
            resume_token: None,
            expires_at: None,
            ws_url: None,
        }),
        AccessStatus::Expired => Some(Decision {
            status: DecisionStatus::Expired,
            jwt: None,
            session_id: None,
            user_id: None,
            workspace_id: None,
            resume_token: None,
            expires_at: None,
            ws_url: None,
        }),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn ct_hash_eq(stored: &[u8; 32], token: &[u8]) -> bool {
    let got = sha256(token);
    stored.ct_eq(&got).into()
}

fn hex_encode(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 0xf) as usize] as char);
    }
    s
}

fn load_denylist(path: &Path, now: u64) -> Vec<(String, u64)> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut bits = line.split_whitespace();
        match (bits.next(), bits.next().and_then(|s| s.parse::<u64>().ok())) {
            (Some(id), Some(exp)) if exp > now => out.push((id.to_owned(), exp)),
            _ => {}
        }
    }
    out
}

fn append_deny(path: &Path, session_id: &str, exp: u64) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(super::display_err)?;
    writeln!(f, "{session_id} {exp}").map_err(super::display_err)
}

/// Parse `tinker_admin` from a Cookie header.
#[must_use]
pub fn cookie_value(header: Option<&str>) -> Option<&str> {
    let header = header?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("tinker_admin=") {
            return Some(v);
        }
    }
    None
}

/// `Set-Cookie` value for a new admin session.
#[must_use]
pub fn set_cookie(token: &str, https: bool) -> String {
    let mut s = format!(
        "{}={token}; HttpOnly; SameSite=Strict; Path=/",
        Admission::cookie_name()
    );
    if https {
        s.push_str("; Secure");
    }
    s
}

/// Clear-cookie header.
#[must_use]
pub fn clear_cookie(https: bool) -> String {
    let mut s = format!(
        "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        Admission::cookie_name()
    );
    if https {
        s.push_str("; Secure");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrate::config::Config;
    use crate::orchestrate::password::{hash_password, test_other_password, test_password};
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tinker_protocol::ApproveBody;

    struct FixedClock(StdMutex<u64>);
    impl Clock for FixedClock {
        fn unix_seconds(&self) -> u64 {
            *self.0.lock().expect("c")
        }
    }

    struct SeqEntropy(StdMutex<u64>);
    impl Entropy for SeqEntropy {
        fn fill(&self, buf: &mut [u8]) {
            let mut n = self.0.lock().expect("e");
            *n = n.wrapping_add(1);
            let bytes = n.to_le_bytes();
            for (i, b) in buf.iter_mut().enumerate() {
                *b = bytes[i % 8].wrapping_add(i as u8);
            }
        }
    }

    fn scratch() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("t")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tinker-adm-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("d");
        dir
    }

    fn cfg(dir: &Path, now_hash: &str) -> Config {
        Config {
            public_listen: "127.0.0.1:0".parse().expect("p"),
            admin_listen: "127.0.0.1:0".parse().expect("a"),
            admin_password_hash: now_hash.to_owned(),
            jwt_hs256_secret: b"admission-secret".to_vec(),
            default_ttl_seconds: 3600,
            max_ttl_seconds: 604_800,
            pending_ttl_seconds: 30,
            revoke_deny_file: dir.join("deny"),
        }
    }

    fn adm(clock: u64) -> Admission {
        let dir = scratch();
        let hash = hash_password(&test_password()).expect("h");
        Admission::new(
            cfg(&dir, &hash),
            Box::new(FixedClock(StdMutex::new(clock))),
            Box::new(SeqEntropy(StdMutex::new(1))),
        )
        .expect("adm")
    }

    #[test]
    fn apply_approve_poll_deny_expire_revoke() {
        let a = adm(1_000);
        let name = DisplayName::new("Ada").expect("n");
        let r = a.apply(name.clone(), "1.1.1.1").expect("apply");
        let (row, dec) = a
            .poll(r.request_id.as_str(), r.wait_token.as_str())
            .expect("poll");
        assert_eq!(row.status, AccessStatus::Pending);
        assert!(dec.is_none());
        assert!(a.poll(r.request_id.as_str(), "nope").is_err());
        assert!(
            a.poll("deadbeefdeadbeefdeadbeefdeadbeef", r.wait_token.as_str())
                .is_err()
        );
        let body = ApproveBody::new(Some(60)).expect("ttl");
        let d = a.approve(r.request_id.as_str(), body).expect("ok");
        assert_eq!(d.status, DecisionStatus::Approved);
        assert!(d.user_id.is_some() && d.workspace_id.is_some() && d.jwt.is_some());
        let (_, dec) = a
            .poll(r.request_id.as_str(), r.wait_token.as_str())
            .expect("p2");
        assert_eq!(dec.expect("d").status, DecisionStatus::Approved);
        assert!(a.approve(r.request_id.as_str(), body).is_err());
        assert!(
            a.approve(
                "deadbeefdeadbeefdeadbeefdeadbeef",
                ApproveBody::new(Some(60)).expect("t")
            )
            .is_err()
        );
        let sid = d.session_id.expect("sid");
        a.revoke(sid.as_str()).expect("rev");
        assert!(a.is_revoked(sid.as_str(), 1_000));
        assert!(!a.is_revoked(sid.as_str(), 10_000_000));
        assert!(a.revoke("missingmissingmissingmissingmiss").is_err());

        let r2 = a.apply(name.clone(), "1.1.1.1").expect("a2");
        a.deny(r2.request_id.as_str()).expect("deny");
        let (_, dec) = a
            .poll(r2.request_id.as_str(), r2.wait_token.as_str())
            .expect("pd");
        assert_eq!(dec.expect("x").status, DecisionStatus::Denied);
        assert!(a.deny(r2.request_id.as_str()).is_err());
        assert!(a.deny("deadbeefdeadbeefdeadbeefdeadbeef").is_err());

        let r3 = a.apply(name, "2.2.2.2").expect("a3");
        assert_eq!(a.list_requests().len(), 3);
        let _ = r3;

        let clock = std::sync::Arc::new(StdMutex::new(1_000u64));
        struct Shared(std::sync::Arc<StdMutex<u64>>);
        impl Clock for Shared {
            fn unix_seconds(&self) -> u64 {
                *self.0.lock().expect("c")
            }
        }
        let dir = scratch();
        let hash = hash_password(&test_password()).expect("h");
        let b = Admission::new(
            cfg(&dir, &hash),
            Box::new(Shared(clock.clone())),
            Box::new(SeqEntropy(StdMutex::new(80))),
        )
        .expect("b");
        let r4 = b
            .apply(DisplayName::new("Bo").expect("n"), "3.3.3.3")
            .expect("a4");
        *clock.lock().expect("c") = 1_040;
        let (row, dec) = b
            .poll(r4.request_id.as_str(), r4.wait_token.as_str())
            .expect("exp");
        assert_eq!(row.status, AccessStatus::Expired);
        assert_eq!(dec.expect("e").status, DecisionStatus::Expired);
        assert!(
            b.approve(r4.request_id.as_str(), ApproveBody::new(None).expect("t"))
                .is_err()
        );
        assert!(!b.list_requests().is_empty());
        assert!(ApproveBody::new(Some(b.config().max_ttl_seconds + 1)).is_err());
        let over = ApproveBody::new(Some(60)).expect("t");
        // max is 604800; 60 is fine. Force over via config max 10:
        let dir = scratch();
        let hash = hash_password(&test_password()).expect("h");
        let mut c = cfg(&dir, &hash);
        c.max_ttl_seconds = 10;
        let c_adm = Admission::new(
            c,
            Box::new(FixedClock(StdMutex::new(1_000))),
            Box::new(SeqEntropy(StdMutex::new(3))),
        )
        .expect("c");
        let rr = c_adm
            .apply(DisplayName::new("C").expect("n"), "4.4.4.4")
            .expect("a");
        assert!(
            c_adm
                .approve(
                    rr.request_id.as_str(),
                    ApproveBody::new(Some(60)).expect("t")
                )
                .is_err()
        );
        assert_eq!(over.effective_ttl(), 60);
    }

    #[test]
    fn login_cookie_rate_and_deny_file() {
        let a = adm(5_000);
        assert!(!a.admin_ok(None));
        assert!(!a.admin_ok(Some("nope")));
        assert!(a.login(&test_other_password(), "9.9.9.9", false).is_err());
        let tok = a.login(&test_password(), "9.9.9.9", true).expect("ok");
        assert!(a.admin_ok(Some(&tok)));
        a.logout(Some(&tok));
        assert!(!a.admin_ok(Some(&tok)));
        a.logout(None);
        let https = set_cookie("abc", true);
        assert!(https.contains("Secure"));
        assert!(clear_cookie(true).contains("Secure"));
        assert!(!set_cookie("abc", false).contains("Secure"));
        assert!(!clear_cookie(false).contains("Secure"));
        assert_eq!(cookie_value(None), None);
        assert_eq!(
            cookie_value(Some("a=b; tinker_admin=xyz ; z=1")),
            Some("xyz")
        );
        assert_eq!(cookie_value(Some("a=b")), None);
        assert!(load_denylist(Path::new("/no-such-tinker-deny-file"), 1).is_empty());
        assert_eq!(Admission::cookie_name(), "tinker_admin");

        for i in 0..5 {
            let _ = a.login(&test_other_password(), "8.8.8.8", false);
            let _ = i;
        }
        assert_eq!(
            a.login(&test_password(), "8.8.8.8", false),
            Err(AdmitError::RateLimited)
        );

        let dir = scratch();
        let deny = dir.join("nested").join("deny");
        let hash = hash_password(&test_password()).expect("h");
        let mut c = cfg(&dir, &hash);
        c.revoke_deny_file = deny.clone();
        std::fs::create_dir_all(deny.parent().expect("parent")).expect("nested");
        std::fs::write(
            &deny,
            "not-a-pair\n  \nbadline\n0123456789abcdef0123456789abcdef 999999\nexpired 1\n",
        )
        .expect("pre");
        // parent exists from write
        let loaded = Admission::new(
            c.clone(),
            Box::new(FixedClock(StdMutex::new(1))),
            Box::new(SeqEntropy(StdMutex::new(1))),
        )
        .expect("load");
        assert!(loaded.is_revoked("0123456789abcdef0123456789abcdef", 1));
        drop(loaded);
        c.revoke_deny_file = dir.join("newdir").join("deny");
        Admission::new(
            c,
            Box::new(FixedClock(StdMutex::new(1))),
            Box::new(SeqEntropy(StdMutex::new(1))),
        )
        .expect("create parent");

        let mut rel = cfg(&dir, &hash);
        rel.revoke_deny_file = PathBuf::from("tinker-rel-deny");
        Admission::new(
            rel,
            Box::new(FixedClock(StdMutex::new(1))),
            Box::new(SeqEntropy(StdMutex::new(1))),
        )
        .expect("relative deny");
        let _ = std::fs::remove_file("tinker-rel-deny");

        let blocked_parent = scratch().join("as-file");
        std::fs::write(&blocked_parent, b"x").expect("file");
        let mut c3 = cfg(&scratch(), &hash);
        c3.revoke_deny_file = blocked_parent.join("deny");
        assert!(
            Admission::new(
                c3,
                Box::new(FixedClock(StdMutex::new(1))),
                Box::new(SeqEntropy(StdMutex::new(1))),
            )
            .is_err()
        );

        let ro = scratch().join("ro");
        std::fs::create_dir(&ro).expect("ro");
        let mut perms = std::fs::metadata(&ro).expect("meta").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o555);
        }
        #[cfg(not(unix))]
        {
            perms.set_readonly(true);
        }
        std::fs::set_permissions(&ro, perms).expect("chmod");
        let mut c4 = cfg(&scratch(), &hash);
        c4.revoke_deny_file = ro.join("deny");
        let created = Admission::new(
            c4,
            Box::new(FixedClock(StdMutex::new(1))),
            Box::new(SeqEntropy(StdMutex::new(1))),
        );
        let mut perms = std::fs::metadata(&ro).expect("meta").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
        }
        let _ = std::fs::set_permissions(&ro, perms);
        assert!(created.is_err());

        let blocked = PathBuf::from("/proc/1/not-a-dir-we-can-create/deny");
        let mut c2 = cfg(&scratch(), &hash);
        c2.revoke_deny_file = blocked;
        // may fail create — ignore if environment allows
        let _ = Admission::new(
            c2,
            Box::new(FixedClock(StdMutex::new(1))),
            Box::new(SeqEntropy(StdMutex::new(1))),
        );
    }

    #[test]
    fn apply_flood() {
        let a = adm(7_000);
        let name = DisplayName::new("Ada").expect("n");
        for _ in 0..10 {
            a.apply(name.clone(), "5.5.5.5").expect("ok");
        }
        assert_eq!(a.apply(name, "5.5.5.5"), Err(AdmitError::RateLimited));
    }

    #[test]
    fn pending_cap_idle_window_and_revoke_io() {
        let clock = std::sync::Arc::new(StdMutex::new(9_000u64));
        struct Shared(std::sync::Arc<StdMutex<u64>>);
        impl Clock for Shared {
            fn unix_seconds(&self) -> u64 {
                *self.0.lock().expect("c")
            }
        }
        let dir = scratch();
        let hash = hash_password(&test_password()).expect("h");
        let a = Admission::new(
            cfg(&dir, &hash),
            Box::new(Shared(clock.clone())),
            Box::new(SeqEntropy(StdMutex::new(4))),
        )
        .expect("a");
        let name = DisplayName::new("Ada").expect("n");
        for i in 0..30 {
            a.apply(name.clone(), &format!("10.0.0.{i}")).expect("p");
        }
        assert_eq!(
            a.apply(name.clone(), "10.0.0.99"),
            Err(AdmitError::RateLimited)
        );

        *clock.lock().expect("c") = 20_000;
        a.apply(name.clone(), "5.5.5.5").expect("after window");
        for _ in 0..9 {
            a.apply(name.clone(), "5.5.5.5").expect("hour");
        }
        assert_eq!(a.apply(name, "5.5.5.5"), Err(AdmitError::RateLimited));
        *clock.lock().expect("c") = 20_000 + 3600;
        a.apply(DisplayName::new("Eve").expect("n"), "5.5.5.5")
            .expect("hour reset");

        for _ in 0..5 {
            let _ = a.login(&test_other_password(), "7.7.7.7", false);
        }
        assert_eq!(
            a.login(&test_password(), "7.7.7.7", false),
            Err(AdmitError::RateLimited)
        );
        *clock.lock().expect("c") = 20_000 + 3600 + 60;
        let tok = a
            .login(&test_password(), "7.7.7.7", false)
            .expect("after minute");
        assert!(a.admin_ok(Some(&tok)));
        *clock.lock().expect("c") = 20_000 + 3600 + 60 + 8 * 3600 + 1;
        assert!(!a.admin_ok(Some(&tok)));

        let mut c = cfg(&scratch(), &hash);
        c.revoke_deny_file = dir.clone();
        let blocked = Admission::new(
            c,
            Box::new(FixedClock(StdMutex::new(1_000))),
            Box::new(SeqEntropy(StdMutex::new(9))),
        )
        .expect("dir deny");
        let rr = blocked
            .apply(DisplayName::new("Z").expect("n"), "1.2.3.4")
            .expect("a");
        let d = blocked
            .approve(
                rr.request_id.as_str(),
                ApproveBody::new(Some(60)).expect("t"),
            )
            .expect("ok");
        assert!(blocked.revoke(d.session_id.expect("s").as_str()).is_err());

        let clock = std::sync::Arc::new(StdMutex::new(1_000u64));
        let dir = scratch();
        std::fs::write(dir.join("deny"), "stale 1001\n").expect("deny");
        let mut c = cfg(&dir, &hash);
        c.revoke_deny_file = dir.join("deny");
        let aged = Admission::new(
            c,
            Box::new(Shared(clock.clone())),
            Box::new(SeqEntropy(StdMutex::new(11))),
        )
        .expect("aged");
        let rr = aged
            .apply(DisplayName::new("Y").expect("n"), "8.8.8.8")
            .expect("a");
        let d = aged
            .approve(
                rr.request_id.as_str(),
                ApproveBody::new(Some(60)).expect("t"),
            )
            .expect("ok");
        *clock.lock().expect("c") = 2_000;
        aged.revoke(d.session_id.expect("s").as_str())
            .expect("drop stale");
    }

    #[test]
    fn system_clock_and_os_entropy() {
        let t = SystemClock.unix_seconds();
        assert!(t >= 1_700_000_000);
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        OsEntropy.fill(&mut a);
        OsEntropy.fill(&mut b);
        assert_ne!(a, b);
    }
}
