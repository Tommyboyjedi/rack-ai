//! Fixed adapter safety policy; lifecycle/model policy is administrator configuration.
pub const JSON_BODY_BYTES: usize = 16 * 1024;
pub const LOGIN_BODY_BYTES: usize = 8 * 1024;
pub const NATIVE_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
pub const NATIVE_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub const SOCKET_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
pub const HTTP_CONCURRENCY: usize = 64;
// Each native request owns a blocking worker, HTTP-client thread and bounded
// machine probes. Leave headroom under the media unit TasksMax=256 for controls
// and the supervisor; queue browser asset bursts before starting those probes.
pub const NATIVE_CONCURRENCY: usize = 16;
pub const SOCKET_CONCURRENCY: usize = 16;
pub const ACTIVE_JOBS: usize = 128;
pub const RETAINED_RECORDS: usize = 2048;
pub const BROWSER_SESSIONS: usize = 64;
pub const COOKIE_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const SOCKET_SECONDS: u64 = 3600;
pub const AUTHORITY_SECONDS: u64 = 20;
pub const HEARTBEAT_SECONDS: u64 = 2;
pub const IMAGE_MAX_EDGE: u32 = 1024;
pub const NATIVE_WAITERS: usize = 128;
// Allow the bounded asset queue to drain at the native task budget.
pub const NATIVE_WAIT_SECONDS: u64 = 15;

pub const ACCOUNT_BODY_BYTES: usize = 16 * 1024;

pub const BROWSER_SOCKET_AUTH_SECONDS: u64 = 2;
