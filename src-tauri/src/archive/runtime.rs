//! Task cancellation and request pacing shared by every archive HTTP attempt.
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};

use reqwest::{header::RETRY_AFTER, Response, StatusCode};
use tokio::sync::{watch, Mutex};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Run,
    Pause,
    Abandon,
}

impl Control {
    fn error(self) -> Option<String> {
        match self {
            Self::Run => None,
            Self::Pause => Some("ARCHIVE_PAUSED".into()),
            Self::Abandon => Some("ARCHIVE_ABANDONED".into()),
        }
    }
}

pub struct RequestGate {
    next: Mutex<Instant>,
    failures: AtomicUsize,
}

impl Default for RequestGate {
    fn default() -> Self {
        Self {
            next: Mutex::new(Instant::now()),
            failures: AtomicUsize::new(0),
        }
    }
}

#[derive(Clone)]
pub struct Session {
    app: tauri::AppHandle,
    pub owner: String,
    control: watch::Receiver<Control>,
    gate: Arc<RequestGate>,
    interval: Duration,
    visible_endpoint: Arc<AtomicUsize>,
    page_token: Arc<tokio::sync::OnceCell<Option<String>>>,
}

impl Session {
    pub fn new(
        app: tauri::AppHandle,
        owner: String,
        control: watch::Receiver<Control>,
        gate: Arc<RequestGate>,
        interval_ms: u64,
    ) -> Self {
        Self {
            app,
            owner,
            control,
            gate,
            interval: Duration::from_millis(interval_ms.clamp(2_000, 30_000)),
            visible_endpoint: Arc::new(AtomicUsize::new(2)),
            page_token: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    pub async fn page_token(
        &self,
        fetch: impl Future<Output = Result<Option<String>, String>>,
    ) -> Result<Option<String>, String> {
        self.page_token.get_or_try_init(|| fetch).await.cloned()
    }

    pub fn visible_endpoint(&self) -> Option<usize> {
        let index = self.visible_endpoint.load(Ordering::Relaxed);
        (index < 2).then_some(index)
    }
    pub fn select_visible_endpoint(&self, index: usize) {
        self.visible_endpoint.store(index, Ordering::Relaxed);
    }

    pub fn check(&self) -> Result<(), String> {
        self.control.borrow().error().map_or(Ok(()), Err)
    }

    pub fn validate_owner(&self, owner: &str) -> Result<(), String> {
        self.check()?;
        if self.owner != owner {
            return Err("ARCHIVE_ACCOUNT_CHANGED:登录账号已变化，请切回原账号继续".into());
        }
        Ok(())
    }

    /// Cancel only network futures. Database commits must always run to completion.
    pub async fn network<T>(
        &self,
        future: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        interruptible(self.control.clone(), future).await
    }

    pub async fn before_request(&self) -> Result<(), String> {
        self.check()?;
        let mut next = self.gate.next.lock().await;
        tokio::time::sleep_until(*next).await;
        self.check()?;
        let app = self.app.clone();
        let owner = self.owner.clone();
        let retry_at = super::blocking(move || super::reserve_archive_page(&app, &owner)).await?;
        if let Some(retry_at) = retry_at {
            return Err(format!("ARCHIVE_RATE_LIMIT:{retry_at}"));
        }
        // Reserve actual HTTP attempts, including fallback endpoints and retries.
        *next = Instant::now() + self.interval;
        Ok(())
    }

    pub async fn observe(&self, response: &Response) -> Result<(), String> {
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(format!(
                "ARCHIVE_AUTH_REQUIRED:HTTP {status}，请检查登录状态或访问权限"
            ));
        }
        if status.is_success() {
            self.gate.failures.store(0, Ordering::Relaxed);
        } else if status.is_server_error() {
            let failures = self.gate.failures.fetch_add(1, Ordering::Relaxed).min(4) as u32;
            let delay = self
                .interval
                .saturating_mul(2_u32.pow(failures + 1))
                .min(Duration::from_secs(30));
            let mut next = self.gate.next.lock().await;
            *next = (*next).max(Instant::now() + delay);
        }
        if status != StatusCode::TOO_MANY_REQUESTS && status != StatusCode::SERVICE_UNAVAILABLE {
            return Ok(());
        }
        let retry = retry_after(
            response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            SystemTime::now(),
        );
        if status == StatusCode::SERVICE_UNAVAILABLE && retry.is_none() {
            return Ok(());
        }
        let duration = retry
            .unwrap_or(Duration::from_secs(60))
            .max(Duration::from_secs(1));
        *self.gate.next.lock().await = Instant::now() + duration;
        let retry_at = super::now().saturating_add(duration.as_secs().min(i64::MAX as u64) as i64);
        let app = self.app.clone();
        let owner = self.owner.clone();
        super::blocking(move || {
            let db = super::open_database(&app)?;
            db.execute(
                "INSERT INTO archive_cooldowns(owner_uin,retry_at) VALUES (?1,?2)
                ON CONFLICT(owner_uin) DO UPDATE SET retry_at=MAX(retry_at,excluded.retry_at)",
                rusqlite::params![owner, retry_at],
            )
            .map_err(|e| format!("保存接口冷却时间失败：{e}"))?;
            Ok(())
        })
        .await?;
        Err(format!("ARCHIVE_RATE_LIMIT:{retry_at}"))
    }
}

async fn interruptible<T>(
    mut control: watch::Receiver<Control>,
    future: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if let Some(error) = control.borrow().error() {
        return Err(error);
    }
    tokio::select! {
        biased;
        signal=async {control.wait_for(|value| *value!=Control::Run).await.map(|value| *value)} => {
            Err(signal.ok().and_then(Control::error).unwrap_or_else(|| "归档控制通道已关闭".into()))
        },
        result=future=>result,
    }
}

fn retry_after(value: Option<&str>, now: SystemTime) -> Option<Duration> {
    let value = value?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(value)
        .ok()
        .map(|date| date.duration_since(now).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn honors_both_retry_after_formats() {
        let now = httpdate::parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT").unwrap();
        assert_eq!(
            retry_after(Some("120"), now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:30:00 GMT"), now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:27:00 GMT"), now),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_after(Some("invalid"), now), None);
    }
    #[tokio::test]
    async fn pause_interrupts_a_pending_request_without_waiting_for_timeout() {
        let (tx, rx) = watch::channel(Control::Run);
        let job = tokio::spawn(interruptible(
            rx,
            std::future::pending::<Result<(), String>>(),
        ));
        tokio::task::yield_now().await;
        tx.send_replace(Control::Pause);
        assert_eq!(job.await.unwrap(), Err("ARCHIVE_PAUSED".into()));
    }
    #[tokio::test]
    async fn abandon_wins_over_an_already_ready_network_result() {
        let (tx, rx) = watch::channel(Control::Run);
        tx.send_replace(Control::Abandon);
        assert_eq!(
            interruptible(rx, async { Ok(()) }).await,
            Err("ARCHIVE_ABANDONED".into())
        );
    }
}
