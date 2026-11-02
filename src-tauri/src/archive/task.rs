//! Ordered source orchestration; request pacing and checkpoint commits are separate.
use super::*;

pub(super) async fn run_feed_archive(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    archive: tauri::State<'_, ArchiveState>,
    session: Session,
) -> Result<ArchiveProgress, String> {
    let owner_uin = session.owner.clone();
    let db_app = app.clone();
    let db_owner = owner_uin.clone();
    let (history, mut skipped) = blocking(move || {
        let db = open_database(&db_app)?;
        Ok((
            checkpoint::read(&db, &db_owner, "history")?,
            read_unresolved_skip_count(&db, &db_owner)?,
        ))
    })
    .await?;
    set_progress(&archive, |p| {
        p.skipped = skipped;
        p.pages = history.pages;
        p.fetched = history.records;
        p.saved = history.saved;
        p.stage = "history";
        p.message = "已恢复历史说说归档进度".into();
    });
    let mut stopped = match sync_history_messages(&app, &login, &archive, &session, None).await {
        Err(error) => error,
        Ok(never) => match never {},
    };
    if let Err(error) = session.check() {
        stopped = error;
    }
    let db_app = app.clone();
    let db_owner = owner_uin.clone();
    skipped = blocking(move || unresolved_skip_count(&db_app, &db_owner)).await?;
    let abandon = *archive.control.borrow() == Control::Abandon;
    let db_app = app.clone();
    let db_owner = owner_uin.clone();
    blocking(move || {
        let mut db = open_database(&db_app)?;
        let complete = !abandon && load_archive_completion(&db, &db_owner)?;
        finalize_archive(&mut db, &db_owner, complete, abandon)?;
        Ok(())
    })
    .await?;
    set_progress(&archive, |p| {
        p.complete = false;
        p.skipped = skipped;
        p.retry_at = None;
        match stopped.as_str() {
            "ARCHIVE_PAUSED" => {
                p.status = "paused";
                p.message = "已暂停，已保存数据和各阶段断点".into();
            }
            "ARCHIVE_ABANDONED" => {
                p.status = "abandoned";
                p.message = "已放弃本次任务，保留已归档内容；再次启动将重新扫描".into();
            }
            error if error.starts_with("ARCHIVE_RATE_LIMIT:") => {
                p.status = "limited";
                p.retry_at = error.trim_start_matches("ARCHIVE_RATE_LIMIT:").parse().ok();
                p.message = "请求已进入冷却，断点已保存；倒计时结束后可继续".into();
            }
            error if error.starts_with("ARCHIVE_AUTH_EXPIRED:") => {
                p.status = "auth_required";
                p.message = error.trim_start_matches("ARCHIVE_AUTH_EXPIRED:").into();
            }
            error => {
                p.status = "error";
                p.message = format!("归档已停止，进度已保留：{}", concise_archive_error(error));
            }
        }
    });
    archive
        .progress
        .lock()
        .map(|p| p.clone())
        .map_err(|_| "归档状态锁已损坏".into())
}
