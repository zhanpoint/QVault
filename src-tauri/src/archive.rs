use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::Manager;

use crate::{commands::qlogin::QLoginState, commands::qzone};

#[path = "archive/checkpoint.rs"]
mod checkpoint;
#[path = "archive/contacts.rs"]
pub(crate) mod contacts;
#[path = "archive/gallery.rs"]
pub(crate) mod gallery;
#[path = "archive/library.rs"]
pub(crate) mod library;
#[path = "archive/library_content.rs"]
mod library_content;
#[path = "archive/media.rs"]
mod media;
#[path = "archive/runtime.rs"]
pub(crate) mod runtime;
#[path = "archive/task.rs"]
mod task;
use runtime::{Control, RequestGate, Session};

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("归档本地任务失败：{e}"))?
}

#[derive(Clone, Copy, Default, serde::Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveMode {
    #[default]
    Own,
    Recovery,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveProgress {
    owner_uin: Option<String>,
    mode: ArchiveMode,
    task_id: Option<String>,
    stage: &'static str,
    active: bool,
    status: &'static str,
    pages: u32,
    fetched: u64,
    saved: u64,
    archived: u64,
    skipped: u32,
    message: String,
    retry_at: Option<i64>,
    can_resume: bool,
    complete: bool,
}

impl Default for ArchiveProgress {
    fn default() -> Self {
        Self {
            owner_uin: None,
            mode: ArchiveMode::Own,
            task_id: None,
            stage: "visible",
            active: false,
            status: "idle",
            pages: 0,
            fetched: 0,
            saved: 0,
            archived: 0,
            skipped: 0,
            message: "尚未开始归档".into(),
            retry_at: None,
            can_resume: false,
            complete: false,
        }
    }
}

pub struct ArchiveState {
    progress: Mutex<ArchiveProgress>,
    task: Arc<tokio::sync::Mutex<()>>,
    control: tokio::sync::watch::Sender<Control>,
    requests: Arc<RequestGate>,
    media_downloads: tokio::sync::Semaphore,
    media_locks: media::DownloadLocks,
    media_client: reqwest::Client,
    library_media_client: reqwest::Client,
}

impl ArchiveState {
    pub fn new() -> Self {
        Self {
            progress: Mutex::new(ArchiveProgress::default()),
            task: Arc::new(tokio::sync::Mutex::new(())),
            control: tokio::sync::watch::channel(Control::Run).0,
            requests: Arc::new(RequestGate::default()),
            media_downloads: tokio::sync::Semaphore::new(4),
            media_locks: media::DownloadLocks::default(),
            library_media_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(20))
                .read_timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("library media HTTP client"),
            media_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::limited(5))
                .connect_timeout(std::time::Duration::from_secs(20))
                .read_timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("media HTTP client"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveItem {
    #[serde(skip)]
    owner_uin: String,
    id: i64,
    cell_id: String,
    published_at: i64,
    content: Option<String>,
    author_uin: Option<String>,
    author_name: Option<String>,
    picture_urls: Vec<String>,
    video_urls: Vec<String>,
    like_count: i64,
    comment_count: i64,
    likes: Vec<LikeUser>,
    comments: Vec<ArchiveComment>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveComment {
    #[serde(skip)]
    comment_id: Option<String>,
    uin: Option<String>,
    nickname: Option<String>,
    content: String,
    created_at: i64,
    replies: Vec<ArchiveReply>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveReply {
    uin: Option<String>,
    nickname: Option<String>,
    reply_to_uin: Option<String>,
    reply_to_nickname: Option<String>,
    content: String,
    created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeUser {
    uin: Option<String>,
    nickname: Option<String>,
    historical: bool,
    liked_at: i64,
}

struct ParsedFeed {
    feed_key: String,
    cell_id: Option<String>,
    event_type: i64,
    event_time: i64,
    title: Option<String>,
    content: Option<String>,
    event_summary: Option<String>,
    actor_uin: Option<String>,
    actor_name: Option<String>,
    original_author_uin: Option<String>,
    original_author_name: Option<String>,
    picture_count: i64,
    pictures_json: Option<String>,
    video_json: Option<String>,
    comments_json: Option<String>,
    raw_json: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSkipItem {
    id: i64,
    page_number: u32,
    cursor_offset: i64,
    offset_advance: i64,
    base_time: i64,
    error: String,
    skipped_at: i64,
    retry_count: u32,
    last_retry_at: Option<i64>,
    resolved_at: Option<i64>,
    recovered_records: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSkipRetryResult {
    success: bool,
    message: String,
    recovered_records: u64,
}

fn stable_feed_hash(value: &Value) -> u64 {
    // FNV-1a keeps fallback keys deterministic without adding a hashing dependency.
    value
        .to_string()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

fn database_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法获取应用数据目录：{error}"))?;
    fs::create_dir_all(&dir).map_err(|error| format!("无法创建应用数据目录：{error}"))?;
    Ok(dir.join("qzone-archive.sqlite3"))
}

fn open_database(app: &tauri::AppHandle) -> Result<Connection, String> {
    let mut connection = Connection::open(database_path(app)?)
        .map_err(|error| format!("无法打开归档数据库：{error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("设置数据库等待时间失败：{e}"))?;
    initialize_database(&mut connection)?;
    Ok(connection)
}

fn initialize_database(connection: &mut Connection) -> Result<(), String> {
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|e| format!("启用数据库外键失败：{e}"))?;
    const SCHEMA_VERSION: i64 = 2;
    static SCHEMA_LOCK: Mutex<()> = Mutex::new(());
    let version = |db: &Connection| {
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .map_err(|e| format!("读取数据库结构标识失败：{e}"))
    };
    if version(connection)? >= SCHEMA_VERSION {
        return Ok(());
    }
    // Serialize first opens; publish the schema marker only after all DDL succeeds.
    let _guard = SCHEMA_LOCK.lock().map_err(|_| "数据库初始化锁已损坏")?;
    if version(connection)? >= SCHEMA_VERSION {
        return Ok(());
    }
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("启用数据库 WAL 失败：{e}"))?;
    let transaction = connection
        .transaction()
        .map_err(|e| format!("开始归档数据库初始化失败：{e}"))?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS archive_stage_checkpoints (
           owner_uin TEXT NOT NULL,stage TEXT NOT NULL,snapshot TEXT NOT NULL,
           PRIMARY KEY(owner_uin,stage)
         );
         CREATE TABLE IF NOT EXISTS archive_cooldowns (
           owner_uin TEXT PRIMARY KEY,retry_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS archive_feeds (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           owner_uin TEXT NOT NULL,
           feed_key TEXT NOT NULL,
           cell_id TEXT,
           event_type INTEGER NOT NULL DEFAULT 0,
           event_time INTEGER NOT NULL DEFAULT 0,
           title TEXT,
           content TEXT,
           event_summary TEXT,
           actor_uin TEXT,
           actor_name TEXT,
           original_author_uin TEXT,
           original_author_name TEXT,
           picture_count INTEGER NOT NULL DEFAULT 0,
           pictures_json TEXT,
           video_json TEXT,
           comments_json TEXT,
           raw_json TEXT NOT NULL,
           archived_at INTEGER NOT NULL,
           UNIQUE(owner_uin, feed_key)
         );
         CREATE INDEX IF NOT EXISTS idx_archive_feeds_owner_time
           ON archive_feeds(owner_uin, event_time DESC);
         CREATE INDEX IF NOT EXISTS idx_archive_feeds_dynamic_type
           ON archive_feeds(owner_uin, cell_id, event_type);
         CREATE TABLE IF NOT EXISTS archive_dynamics (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           owner_uin TEXT NOT NULL,
           cell_id TEXT NOT NULL,
           published_at INTEGER NOT NULL DEFAULT 0,
           content TEXT,
           author_uin TEXT,
           author_name TEXT,
           category TEXT NOT NULL DEFAULT '',
           pictures_json TEXT,
           video_json TEXT,
           raw_original_json TEXT NOT NULL,
           archived_at INTEGER NOT NULL,
           UNIQUE(owner_uin, cell_id)
         );
         CREATE INDEX IF NOT EXISTS idx_archive_dynamics_owner_time
           ON archive_dynamics(owner_uin, published_at DESC);
         CREATE TABLE IF NOT EXISTS archive_checkpoints (
           owner_uin TEXT PRIMARY KEY,
           attach_info TEXT NOT NULL,
           pages INTEGER NOT NULL DEFAULT 0,
           fetched INTEGER NOT NULL DEFAULT 0,
           saved INTEGER NOT NULL DEFAULT 0,
           updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS archive_completions (
           owner_uin TEXT PRIMARY KEY,
           completed_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS archive_rate_limits (
           owner_uin TEXT PRIMARY KEY,
           window_started_at INTEGER NOT NULL,
           requested_pages INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS archive_skips (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           owner_uin TEXT NOT NULL,
           cursor TEXT NOT NULL,
           resume_cursor TEXT NOT NULL,
           page_number INTEGER NOT NULL,
           cursor_offset INTEGER NOT NULL,
           offset_advance INTEGER NOT NULL,
           base_time INTEGER NOT NULL,
           error TEXT NOT NULL,
           skipped_at INTEGER NOT NULL,
           retry_count INTEGER NOT NULL DEFAULT 0,
           last_retry_at INTEGER,
           resolved_at INTEGER,
           recovered_records INTEGER NOT NULL DEFAULT 0,
           UNIQUE(owner_uin, cursor_offset, base_time)
         );
         CREATE TABLE IF NOT EXISTS archive_library_items (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           owner_uin TEXT NOT NULL,
           module TEXT NOT NULL,
           scope_key TEXT NOT NULL DEFAULT '',
           item_key TEXT NOT NULL,
           parent_key TEXT NOT NULL DEFAULT '',
           created_at INTEGER NOT NULL DEFAULT 0,
           title TEXT NOT NULL DEFAULT '',
           summary TEXT NOT NULL DEFAULT '',
           author_uin TEXT,
           author_name TEXT,
           cover_url TEXT,
           media_json TEXT NOT NULL DEFAULT '[]',
           raw_json TEXT NOT NULL,
           archived_at INTEGER NOT NULL,
           UNIQUE(owner_uin,module,scope_key,item_key)
         );
         CREATE INDEX IF NOT EXISTS idx_archive_library_module_time
           ON archive_library_items(owner_uin,module,scope_key,created_at DESC);
         CREATE TABLE IF NOT EXISTS archive_library_state (
           owner_uin TEXT NOT NULL,
           module TEXT NOT NULL,
           scope_key TEXT NOT NULL DEFAULT '',
           remote_total INTEGER NOT NULL DEFAULT 0,
           fetched INTEGER NOT NULL DEFAULT 0,
           complete INTEGER NOT NULL DEFAULT 0,
           last_error TEXT,
           synced_at INTEGER NOT NULL DEFAULT 0,
           PRIMARY KEY(owner_uin,module,scope_key)
         );",
        )
        .map_err(|error| format!("初始化归档数据库失败：{error}"))?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| format!("保存数据库结构标识失败：{e}"))?;
    transaction
        .commit()
        .map_err(|e| format!("提交归档数据库初始化失败：{e}"))
}

fn text_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer).and_then(|value| match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn parse_feed(feed: &Value) -> Result<ParsedFeed, String> {
    let cell_id = text_at(feed, "/original/cell_id/cellid");
    let event_time = feed
        .pointer("/comm/time")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let event_type = feed
        .pointer("/comm/subid")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let actor_uin = text_at(feed, "/userinfo/user/uin");
    let feed_key = text_at(feed, "/comm/feedskey")
        .or_else(|| text_at(feed, "/original/cell_comm/feedskey"))
        .or_else(|| {
            cell_id.as_ref().map(|id| {
                format!(
                    "{event_type}:{id}:{event_time}:{}",
                    actor_uin.as_deref().unwrap_or("unknown")
                )
            })
        })
        .unwrap_or_else(|| {
            format!(
                "fallback:{event_type}:{event_time}:{}:{:016x}",
                actor_uin.as_deref().unwrap_or("unknown"),
                stable_feed_hash(feed)
            )
        });
    let pictures = feed.pointer("/original/cell_pic");
    let picture_count = pictures
        .and_then(|value| value.pointer("/picdata/pic"))
        .and_then(Value::as_array)
        .map(|items| items.len() as i64)
        .unwrap_or(0);
    let video = feed
        .pointer("/original/cell_video")
        .filter(|value| !value.is_null());
    let comments = feed
        .pointer("/original/cell_comment")
        .filter(|value| !value.is_null());
    Ok(ParsedFeed {
        feed_key,
        cell_id,
        event_type,
        event_time,
        title: text_at(feed, "/title/title"),
        content: text_at(feed, "/original/cell_summary/summary"),
        event_summary: text_at(feed, "/summary/summary"),
        actor_uin,
        actor_name: text_at(feed, "/userinfo/user/nickname"),
        original_author_uin: text_at(feed, "/original/cell_userinfo/user/uin"),
        original_author_name: text_at(feed, "/original/cell_userinfo/user/nickname"),
        picture_count,
        pictures_json: pictures.map(Value::to_string),
        video_json: video.map(Value::to_string),
        comments_json: comments.map(Value::to_string),
        raw_json: feed.to_string(),
    })
}

// QQ's mood records use appid 311. Author identity alone also matches photos
// and other QQ apps; notification fallback identities do not identify a post.
const OWN_MOOD_PREDICATE: &str = "d.category='self' AND d.author_uin=d.owner_uin
    AND CAST(json_extract(d.raw_original_json,'$.cell_comm.appid') AS INTEGER)=311
    AND d.cell_id NOT LIKE 'history-v3:%'";

fn is_owned_mood(original: &Value, owner: &str) -> bool {
    text_at(original, "/cell_comm/appid").as_deref() == Some("311")
        && text_at(original, "/cell_userinfo/user/uin").as_deref() == Some(owner)
        && text_at(original, "/cell_id/cellid").is_some_and(|id| !id.starts_with("history-v3:"))
}

fn save_feed_rows(
    transaction: &rusqlite::Transaction<'_>,
    owner_uin: &str,
    feeds: &[Value],
) -> Result<u64, String> {
    let mut statement = transaction.prepare_cached(
            "INSERT INTO archive_feeds
             (owner_uin, feed_key, cell_id, event_type, event_time, title, content, event_summary,
              actor_uin, actor_name, original_author_uin, original_author_name, picture_count,
              pictures_json, video_json, comments_json, raw_json, archived_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(owner_uin, feed_key) DO UPDATE SET
              cell_id=excluded.cell_id,event_type=excluded.event_type,event_time=excluded.event_time,
              title=excluded.title,content=excluded.content,event_summary=excluded.event_summary,
              actor_uin=excluded.actor_uin,actor_name=excluded.actor_name,
              original_author_uin=excluded.original_author_uin,original_author_name=excluded.original_author_name,
              picture_count=excluded.picture_count,pictures_json=excluded.pictures_json,
              video_json=excluded.video_json,comments_json=excluded.comments_json,
              raw_json=excluded.raw_json,archived_at=excluded.archived_at").map_err(|e| format!("准备动态写入失败：{e}"))?;
    let ids: Vec<_> = feeds
        .iter()
        .filter_map(|feed| text_at(feed, "/original/cell_id/cellid"))
        .collect();
    let ids_json = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    let mut targets = transaction
        .prepare_cached(&format!(
            "SELECT d.cell_id FROM archive_dynamics d WHERE d.owner_uin=?1 AND {OWN_MOOD_PREDICATE}
         AND d.cell_id IN (SELECT value FROM json_each(?2))"
        ))
        .map_err(|e| format!("准备说说归属查询失败：{e}"))?;
    let mut owned_ids = targets
        .query_map(params![owner_uin, ids_json], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(targets);
    for feed in feeds {
        if let Some(original) = feed
            .get("original")
            .filter(|original| is_owned_mood(original, owner_uin))
        {
            save_original_dynamic(transaction, owner_uin, feed)?;
            owned_ids
                .insert(text_at(original, "/cell_id/cellid").expect("validated mood identity"));
        }
    }
    // Sparse notifications may enrich an already verified post, but cannot create one.
    for feed in feeds {
        if let Some(original) = feed.get("original") {
            if !is_owned_mood(original, owner_uin)
                && text_at(original, "/cell_id/cellid").is_some_and(|id| owned_ids.contains(&id))
            {
                save_original_dynamic(transaction, owner_uin, feed)?;
            }
        }
    }
    let archived_at = now();
    let mut saved = 0;
    for feed in feeds {
        let Some(original) = feed.get("original") else {
            continue;
        };
        if !text_at(original, "/cell_id/cellid").is_some_and(|id| owned_ids.contains(&id))
            || text_at(original, "/cell_comm/appid").is_some_and(|appid| appid != "311")
            || text_at(original, "/cell_userinfo/user/uin")
                .is_some_and(|author| author != owner_uin)
        {
            continue;
        }
        let feed = parse_feed(feed)?;
        let changed = statement
            .execute(params![
                owner_uin,
                feed.feed_key,
                feed.cell_id,
                feed.event_type,
                feed.event_time,
                feed.title,
                feed.content,
                feed.event_summary,
                feed.actor_uin,
                feed.actor_name,
                feed.original_author_uin,
                feed.original_author_name,
                feed.picture_count,
                feed.pictures_json,
                feed.video_json,
                feed.comments_json,
                feed.raw_json,
                archived_at
            ])
            .map_err(|error| format!("保存动态失败：{error}"))?;
        saved += changed as u64;
    }
    Ok(saved)
}

fn save_retried_page(
    connection: &mut Connection,
    owner_uin: &str,
    id: i64,
    feeds: &[Value],
    attempted_at: i64,
) -> Result<u32, String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("无法开始重试事务：{error}"))?;
    save_feed_rows(&transaction, owner_uin, feeds)?;
    transaction
        .execute(
            "UPDATE archive_skips SET retry_count=retry_count+1,last_retry_at=?2,
         resolved_at=?2,recovered_records=?3 WHERE id=?1 AND owner_uin=?4",
            params![id, attempted_at, feeds.len() as i64, owner_uin],
        )
        .map_err(|error| format!("更新异常重试结果失败：{error}"))?;
    let remaining = transaction
        .query_row(
            "SELECT COUNT(*) FROM archive_skips WHERE owner_uin=?1 AND resolved_at IS NULL",
            [owner_uin],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取异常位置数量失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交重试事务失败：{error}"))?;
    Ok(remaining)
}

const ARCHIVE_RATE_WINDOW_SECONDS: i64 = 10 * 60;
const ARCHIVE_RATE_PAGE_LIMIT: i64 = 300;

fn unresolved_skip_count(app: &tauri::AppHandle, owner_uin: &str) -> Result<u32, String> {
    read_unresolved_skip_count(&open_database(app)?, owner_uin)
}

fn read_unresolved_skip_count(connection: &Connection, owner_uin: &str) -> Result<u32, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM archive_skips WHERE owner_uin=?1 AND resolved_at IS NULL",
            params![owner_uin],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取异常跳过数量失败：{error}"))
}

fn reserve_archive_page(app: &tauri::AppHandle, owner_uin: &str) -> Result<Option<i64>, String> {
    let mut db = open_database(app)?;
    reserve_request(&mut db, owner_uin, now())
}

fn reserve_request(db: &mut Connection, owner: &str, current: i64) -> Result<Option<i64>, String> {
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("请求频率事务失败：{e}"))?;
    let cooldown: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(retry_at),0) FROM archive_cooldowns WHERE owner_uin=?1",
            [owner],
            |r| r.get(0),
        )
        .map_err(|e| format!("读取冷却状态失败：{e}"))?;
    if cooldown > current {
        return Ok(Some(cooldown));
    }
    let (start, count): (i64, i64) = tx
        .query_row(
            "SELECT window_started_at,requested_pages FROM archive_rate_limits WHERE owner_uin=?1",
            [owner],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("读取请求预算失败：{e}"))?
        .unwrap_or((current, 0));
    let (start, count) = if current.saturating_sub(start) >= ARCHIVE_RATE_WINDOW_SECONDS {
        (current, 0)
    } else {
        (start, count)
    };
    if count >= ARCHIVE_RATE_PAGE_LIMIT {
        return Ok(Some(start + ARCHIVE_RATE_WINDOW_SECONDS));
    }
    tx.execute("INSERT INTO archive_rate_limits(owner_uin,window_started_at,requested_pages) VALUES (?1,?2,?3) ON CONFLICT(owner_uin) DO UPDATE SET window_started_at=excluded.window_started_at,requested_pages=excluded.requested_pages", params![owner,start,count+1]).map_err(|e| format!("保存请求预算失败：{e}"))?;
    tx.commit().map_err(|e| format!("提交请求预算失败：{e}"))?;
    Ok(None)
}

fn save_archive_completion(
    connection: &Connection,
    owner_uin: &str,
    complete: bool,
) -> Result<(), String> {
    if complete {
        connection
            .execute(
                "INSERT INTO archive_completions(owner_uin,completed_at) VALUES (?1,?2)
             ON CONFLICT(owner_uin) DO UPDATE SET completed_at=excluded.completed_at",
                params![owner_uin, now()],
            )
            .map_err(|error| format!("保存归档完成状态失败：{error}"))?;
    } else {
        connection
            .execute(
                "DELETE FROM archive_completions WHERE owner_uin=?1",
                params![owner_uin],
            )
            .map_err(|error| format!("重置归档完成状态失败：{error}"))?;
    }
    Ok(())
}

fn load_archive_completion(connection: &Connection, owner_uin: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM archive_completions WHERE owner_uin=?1)",
            params![owner_uin],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取归档完成状态失败：{error}"))
}

fn finalize_archive(
    db: &mut Connection,
    owner: &str,
    complete: bool,
    clear_checkpoint: bool,
) -> Result<(), String> {
    let tx = db
        .transaction()
        .map_err(|e| format!("开始归档收尾事务失败：{e}"))?;
    save_archive_completion(&tx, owner, complete)?;
    if clear_checkpoint {
        checkpoint::clear(&tx, owner)?;
    }
    tx.commit()
        .map_err(|e| format!("提交归档收尾事务失败：{e}"))
}

fn save_original_dynamic(
    transaction: &rusqlite::Transaction<'_>,
    owner_uin: &str,
    feed: &Value,
) -> Result<(), String> {
    let Some(original) = feed.get("original") else {
        return Ok(());
    };
    let Some(cell_id) = text_at(original, "/cell_id/cellid") else {
        return Ok(());
    };
    if cell_id.starts_with("history-v3:")
        || text_at(original, "/cell_comm/appid").is_some_and(|appid| appid != "311")
        || text_at(original, "/cell_userinfo/user/uin").is_some_and(|uin| uin != owner_uin)
    {
        return Ok(());
    }
    let verified = is_owned_mood(original, owner_uin);
    if !verified
        && !(original.get("cell_summary").is_some()
            || original.get("cell_pic").is_some()
            || original.get("cell_video").is_some())
    {
        return Ok(());
    }
    let published_at = original
        .pointer("/cell_comm/time")
        .and_then(Value::as_i64)
        .or_else(|| feed.pointer("/comm/time").and_then(Value::as_i64))
        .unwrap_or(0);
    let content = text_at(original, "/cell_summary/summary");
    let author_uin = text_at(original, "/cell_userinfo/user/uin");
    let author_name = text_at(original, "/cell_userinfo/user/nickname");
    let category = "self";
    let previous: Option<(Option<String>, Option<String>, bool)> = transaction.prepare_cached(
        &format!("SELECT pictures_json,video_json,COALESCE(({OWN_MOOD_PREDICATE}),0) FROM archive_dynamics d WHERE owner_uin=?1 AND cell_id=?2"))
        .map_err(|e| e.to_string())?.query_row(params![owner_uin,cell_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))
        .optional().map_err(|e| format!("读取已有媒体失败：{e}"))?;
    let (previous_pictures, previous_video, previously_verified) = previous.unwrap_or_default();
    if !verified && !previously_verified {
        return Ok(());
    }
    let pictures_json = merge_picture_media(
        previous_pictures,
        original
            .get("cell_pic")
            .filter(|v| !v.is_null())
            .map(Value::to_string),
    );
    let video_json = merge_video_media(
        previous_video,
        original
            .get("cell_video")
            .filter(|v| !v.is_null())
            .map(Value::to_string),
    );
    transaction.prepare_cached(
        "INSERT INTO archive_dynamics
         (owner_uin,cell_id,published_at,content,author_uin,author_name,category,pictures_json,video_json,raw_original_json,archived_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(owner_uin,cell_id) DO UPDATE SET
          published_at=CASE
            WHEN archive_dynamics.published_at<=0 THEN excluded.published_at
            WHEN excluded.published_at<=0 THEN archive_dynamics.published_at
            ELSE MIN(archive_dynamics.published_at,excluded.published_at)
          END,
          content=COALESCE(NULLIF(excluded.content,''),archive_dynamics.content),
          author_uin=COALESCE(excluded.author_uin,archive_dynamics.author_uin),
          author_name=COALESCE(NULLIF(excluded.author_name,''),archive_dynamics.author_name),
          category=excluded.category,
          pictures_json=COALESCE(excluded.pictures_json,archive_dynamics.pictures_json),
          video_json=COALESCE(excluded.video_json,archive_dynamics.video_json),
          raw_original_json=CASE WHEN ?12 THEN excluded.raw_original_json ELSE archive_dynamics.raw_original_json END,
          archived_at=excluded.archived_at").map_err(|e| format!("准备原始动态写入失败：{e}"))?.execute(
        params![owner_uin,cell_id,published_at,content,author_uin,author_name,category,pictures_json,video_json,original.to_string(),now(),verified],
    ).map_err(|error| format!("保存原动态失败：{error}"))?;
    Ok(())
}

fn merge_picture_media(previous: Option<String>, incoming: Option<String>) -> Option<String> {
    if previous.is_none() {
        return incoming;
    }
    if incoming.is_none() || previous == incoming {
        return previous;
    }
    let mut pictures = picture_url_candidates(previous);
    for urls in picture_url_candidates(incoming) {
        if let Some(existing) = pictures
            .iter_mut()
            .find(|saved| saved.iter().any(|url| urls.contains(url)))
        {
            let mut latest = urls;
            for url in existing.iter() {
                if !latest.contains(url) {
                    latest.push(url.clone());
                }
            }
            *existing = latest;
        } else {
            pictures.push(urls);
        }
    }
    (!pictures.is_empty()).then(|| json!({"picdata":{"pic": pictures.into_iter()
        .map(|urls| json!({"photourl": urls.into_iter().map(|url| json!({"url":url})).collect::<Vec<_>>()}))
        .collect::<Vec<_>>()}}).to_string())
}

fn merge_video_media(previous: Option<String>, incoming: Option<String>) -> Option<String> {
    if previous.is_none() {
        return incoming;
    }
    if incoming.is_none() || previous == incoming {
        return previous;
    }
    let cover = video_cover_url(previous.clone()).or_else(|| video_cover_url(incoming.clone()));
    let mut urls = video_urls(previous);
    for url in video_urls(incoming) {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    (!urls.is_empty() || cover.is_some()).then(|| json!({"videourls": urls.into_iter()
        .map(|url| json!({"url":url})).collect::<Vec<_>>(), "coverurl": cover.map(|url| json!({"url":url})).into_iter().collect::<Vec<_>>()}).to_string())
}

fn picture_url_candidates(json: Option<String>) -> Vec<Vec<String>> {
    let Some(value) = json.and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return vec![];
    };
    value
        .pointer("/picdata/pic")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pic| {
            let values = match pic.get("photourl") {
                Some(Value::Array(items)) => items.iter().collect::<Vec<_>>(),
                Some(Value::Object(items)) => items.values().collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            let mut candidates = values
                .into_iter()
                .filter_map(|item| {
                    let url = item
                        .as_str()
                        .or_else(|| item.get("url").and_then(Value::as_str))?
                        .trim();
                    if url.is_empty() {
                        return None;
                    }
                    Some(url.to_owned())
                })
                .collect::<Vec<_>>();
            for name in [
                "rawUrl",
                "origin_url",
                "url3",
                "url2",
                "url1",
                "url",
                "smallurl",
            ] {
                if let Some(url) = pic.get(name).and_then(Value::as_str).map(str::trim) {
                    if !url.is_empty() {
                        candidates.push(url.to_owned());
                    }
                }
            }
            if let Some(url) = pic
                .pointer("/busi_param/-1")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|url| !url.is_empty())
            {
                candidates.push(url.to_owned());
            }
            let mut seen = HashSet::new();
            let urls = candidates
                .into_iter()
                .map(|url| {
                    if url.starts_with("//") {
                        format!("https:{url}")
                    } else {
                        url
                    }
                })
                .filter(|url| is_content_media_url(url))
                .filter(|url| seen.insert(url.clone()))
                .collect::<Vec<_>>();
            (!urls.is_empty()).then_some(urls)
        })
        .collect()
}

fn is_content_media_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (url.starts_with("http://") || url.starts_with("https://"))
        && !lower.contains("qlogo")
        && !lower.contains("headimg")
        && !lower.contains("qzonestyle")
        && !lower.contains("custompraise")
}

fn picture_urls(json: Option<String>) -> Vec<String> {
    picture_url_candidates(json)
        .into_iter()
        .filter_map(|urls| urls.into_iter().next())
        .collect()
}

fn archived_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.get(4..12).is_some_and(|value| {
        value.starts_with(b"ftyp") && (&value[4..8] == b"avif" || &value[4..8] == b"avis")
    }) {
        Some("avif")
    } else {
        None
    }
}

fn is_qq_missing_image_placeholder_header(bytes: &[u8], length: u64) -> bool {
    bytes.get(6..10).is_some_and(|size| {
        let width = u16::from_le_bytes([size[0], size[1]]);
        let height = u16::from_le_bytes([size[2], size[3]]);
        (length == 2_038 && bytes.starts_with(b"GIF89a") && width == 340 && height == 320)
            || (length == 2_687 && bytes.starts_with(b"GIF89a") && width == 340 && height == 320)
            || (length == 1_643 && bytes.starts_with(b"GIF87a") && width == 99 && height == 99)
            || (length == 1_547 && bytes.starts_with(b"GIF87a") && width == 98 && height == 98)
    })
}

fn existing_archived_image(image_dir: &std::path::Path, file_stem: &str) -> Option<PathBuf> {
    use std::io::Read;
    ["jpg", "png", "gif", "webp", "avif", "bmp"]
        .into_iter()
        .map(|extension| image_dir.join(format!("{file_stem}.{extension}")))
        .find_map(|path| {
            let mut file = fs::File::open(&path).ok()?;
            let length = file.metadata().ok()?.len();
            let mut prefix = [0; 32];
            if length <= 32
                || file.read_exact(&mut prefix).is_err()
                || archived_image_extension(&prefix).is_none()
                || is_qq_missing_image_placeholder_header(&prefix, length)
            {
                return None;
            }
            Some(path)
        })
}

#[tauri::command]
pub async fn load_archived_image(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    state: tauri::State<'_, ArchiveState>,
    id: i64,
    picture_index: usize,
    force_reload: Option<bool>,
) -> Result<String, String> {
    let auth = login.qzone_auth().await?;
    let image_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法获取图片归档目录：{error}"))?
        .join("images")
        .join(&auth.uin);
    tokio::fs::create_dir_all(&image_dir)
        .await
        .map_err(|error| format!("无法创建图片归档目录：{error}"))?;
    let file_stem = format!("{id}-{picture_index}");
    let _resource = state
        .media_locks
        .acquire(&image_dir.join(&file_stem))
        .await?;
    if !force_reload.unwrap_or(false) {
        let directory = image_dir.clone();
        let stem = file_stem.clone();
        if let Some(path) = blocking(move || Ok(existing_archived_image(&directory, &stem))).await?
        {
            return Ok(path.to_string_lossy().into_owned());
        }
    }

    let db_app = app.clone();
    let db_owner = auth.uin.clone();
    let pictures_json = blocking(move || {
        let connection = open_database(&db_app)?;
        connection
            .query_row(
                "SELECT pictures_json FROM archive_dynamics WHERE id=?1 AND owner_uin=?2",
                params![id, db_owner],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => "当前账号中不存在这条图片归档".into(),
                _ => format!("读取图片归档失败：{error}"),
            })
    })
    .await?;
    let candidates = picture_url_candidates(pictures_json)
        .into_iter()
        .nth(picture_index)
        .ok_or("该图片没有保存可用的 QQ 地址")?;
    let _permit = state
        .media_downloads
        .acquire()
        .await
        .map_err(|_| "图片下载队列已关闭")?;
    let urls = candidates
        .iter()
        .filter_map(|url| gallery::trusted_media_url(url).ok())
        .collect::<Vec<_>>();
    let (download, extension) = media::image(
        &state.media_client,
        &image_dir,
        &urls,
        Some(&auth.desktop_cookie_header),
        &auth.user_agent,
    )
    .await?;
    if login.qzone_auth().await?.cookie_header != auth.cookie_header {
        return Err("登录会话已变化，请重新加载图片".into());
    }
    download
        .publish(image_dir.join(format!("{file_stem}.{extension}")))
        .await
}

#[tauri::command]
pub async fn load_qzone_emoticon(
    app: tauri::AppHandle,
    state: tauri::State<'_, ArchiveState>,
    code: String,
) -> Result<String, String> {
    if !code.strip_prefix('e').is_some_and(|digits| {
        !digits.is_empty() && digits.len() <= 16 && digits.bytes().all(|b| b.is_ascii_digit())
    }) {
        return Err("QQ 表情编码无效".into());
    }
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("emoticons");
    let _lock = state.media_locks.acquire(&directory.join(&code)).await?;
    let cache_dir = directory.clone();
    let cache_code = code.clone();
    if let Some(path) =
        blocking(move || Ok(existing_archived_image(&cache_dir, &cache_code))).await?
    {
        return Ok(path.to_string_lossy().into_owned());
    }
    let _slot = state
        .media_downloads
        .acquire()
        .await
        .map_err(|_| "媒体下载队列已关闭")?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| e.to_string())?;
    let url = url::Url::parse(&format!("https://qzonestyle.gtimg.cn/qzone/em/{code}.gif"))
        .map_err(|e| e.to_string())?;
    let (download, extension) =
        media::image(&state.media_client, &directory, &[url], None, "Mozilla/5.0").await?;
    download
        .publish(directory.join(format!("{code}.{extension}")))
        .await
}

fn video_urls(json: Option<String>) -> Vec<String> {
    let Some(value) = json.and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return vec![];
    };
    fn push(urls: &mut Vec<String>, value: Option<&str>) {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        let url = if value.starts_with("//") {
            format!("https:{value}")
        } else {
            value.to_owned()
        };
        if is_content_media_url(&url) && !urls.iter().any(|saved| saved == &url) {
            urls.push(url);
        }
    }
    fn collect_named(value: &Value, urls: &mut Vec<String>) {
        if let Value::Object(object) = value {
            for name in [
                "videourl",
                "url3",
                "url2",
                "actionurl",
                "playurl",
                "video_url",
                "url",
                "download_url",
            ] {
                push(urls, object.get(name).and_then(Value::as_str));
            }
            if let Some(collection) = object.get("videourls").or_else(|| object.get("urls")) {
                match collection {
                    Value::Array(items) => {
                        for item in items {
                            collect_named(item, urls);
                        }
                    }
                    Value::Object(items) => {
                        for item in items.values() {
                            collect_named(item, urls);
                        }
                    }
                    Value::String(url) => push(urls, Some(url)),
                    _ => {}
                }
            }
        }
    }
    let mut urls = Vec::new();
    collect_named(&value, &mut urls);
    urls
}

fn video_cover_url(json: Option<String>) -> Option<String> {
    let value = json.and_then(|text| serde_json::from_str::<Value>(&text).ok())?;
    value
        .pointer("/coverurl/0/url")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("coverurl")?
                .as_object()?
                .values()
                .find_map(|item| item.get("url")?.as_str())
        })
        .or_else(|| value.get("coverurl").and_then(Value::as_str))
        .or_else(|| value.get("cover_url").and_then(Value::as_str))
        .or_else(|| value.get("cover").and_then(Value::as_str))
        .filter(|url| is_content_media_url(url))
        .map(str::to_owned)
}

#[tauri::command]
pub async fn load_archived_video(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    state: tauri::State<'_, ArchiveState>,
    id: i64,
) -> Result<String, String> {
    let auth = login.qzone_auth().await?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法获取视频缓存目录：{error}"))?
        .join("videos");
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(|error| format!("无法创建视频缓存目录：{error}"))?;
    let cache_path = cache_dir.join(format!("{}-{id}.mp4", auth.uin));
    let _resource = state.media_locks.acquire(&cache_path).await?;
    if cache_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() > 1024)
    {
        return Ok(cache_path.to_string_lossy().into_owned());
    }
    let db_app = app.clone();
    let db_owner = auth.uin.clone();
    let video_json = blocking(move || {
        let connection = open_database(&db_app)?;
        connection
            .query_row(
                "SELECT video_json FROM archive_dynamics WHERE id=?1 AND owner_uin=?2",
                params![id, db_owner],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => "当前账号中不存在这条视频归档".into(),
                _ => format!("读取视频归档失败：{error}"),
            })
    })
    .await?;
    let candidates = video_urls(video_json);
    if candidates.is_empty() {
        return Err("该归档没有可用的视频地址".into());
    }
    let _permit = state
        .media_downloads
        .acquire()
        .await
        .map_err(|_| "媒体下载队列已关闭")?;
    let client = state.media_client.clone();
    let mut last_error = String::new();
    let mut rejected = false;
    for url in candidates {
        for (with_cookie, with_referer) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let mut request = client
                .get(&url)
                .header(reqwest::header::USER_AGENT, &auth.user_agent)
                .header(
                    reqwest::header::ACCEPT,
                    "video/mp4,video/*;q=0.9,application/octet-stream;q=0.8,*/*;q=0.5",
                )
                .header(
                    reqwest::header::ACCEPT_LANGUAGE,
                    "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6,zh-TW;q=0.5",
                );
            if with_cookie {
                request = request.header(reqwest::header::COOKIE, &auth.cookie_header);
            }
            if with_referer {
                request = request.header(reqwest::header::REFERER, "https://user.qzone.qq.com/");
            }
            match request.send().await {
                Ok(response) if response.status().is_success() => {
                    let content_type = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    match media::receive(response, &cache_dir).await {
                        Ok(download) => {
                            let is_mp4 = download
                                .prefix
                                .get(4..12)
                                .is_some_and(|value| value.windows(4).any(|part| part == b"ftyp"));
                            if download.length > 1024
                                && (content_type.starts_with("video/")
                                    || content_type.contains("octet-stream")
                                    || is_mp4)
                            {
                                return download.publish(cache_path).await;
                            }
                            last_error = format!(
                                "QQ 返回了无效的视频内容（{}）",
                                if content_type.is_empty() {
                                    "未知类型"
                                } else {
                                    &content_type
                                }
                            );
                        }
                        Err(error) => last_error = error,
                    }
                }
                Ok(response) => {
                    rejected |= response.status() == reqwest::StatusCode::FORBIDDEN;
                    last_error = format!("HTTP {}", response.status());
                }
                Err(error) => last_error = format!("请求视频失败：{error}"),
            }
        }
    }
    if rejected {
        Err("QQ 拒绝了视频请求（HTTP 403），该归档的视频临时签名可能已经过期，请重新归档以更新视频地址".into())
    } else {
        Err(format!("所有视频地址均加载失败：{last_error}"))
    }
}

fn set_progress(state: &ArchiveState, update: impl FnOnce(&mut ArchiveProgress)) {
    if let Ok(mut progress) = state.progress.lock() {
        update(&mut progress);
    }
}

fn concise_archive_error(error: &str) -> String {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let summary = chars.by_ref().take(240).collect::<String>();
    if chars.next().is_some() {
        format!("{summary}…")
    } else {
        summary
    }
}

async fn sync_visible_moments(
    app: &tauri::AppHandle,
    login: &QLoginState,
    archive: &ArchiveState,
    session: &Session,
) -> Result<(), String> {
    let app_copy = app.clone();
    let owner = session.owner.clone();
    let mut saved = blocking(move || checkpoint::load(&app_copy, &owner, "visible")).await?;
    if saved.complete {
        saved = checkpoint::StageCheckpoint::default();
    }
    set_progress(archive, |p| {
        p.pages = saved.pages;
        p.fetched = saved.records;
        p.saved = saved.saved;
        p.stage = "visible";
        p.message = "正在同步本人说说、评论和回复…".into();
    });
    while !saved.complete {
        session.check()?;
        let pos = saved.offset;
        let page = session
            .network(qzone::fetch_visible_moments(login, pos, 30, Some(session)))
            .await?;
        saved.total = page.total.unwrap_or(saved.total);
        if saved.owner_name.is_none() {
            saved.owner_name = page
                .feeds
                .iter()
                .find_map(|feed| text_at(feed, "/original/cell_userinfo/user/nickname"));
        }
        saved.pages += 1;
        saved.records += page.moment_count as u64;
        saved.offset = page.next_pos;
        saved.complete = !page.has_more;
        let fetched = page.moment_count as u64;
        let previous = saved.saved;
        saved = checkpoint::save(app, &session.owner, "visible", page.feeds, saved).await?;
        set_progress(archive, |p| {
            p.pages += 1;
            p.fetched += fetched;
            p.saved += saved.saved - previous;
            p.message = format!("本人说说已同步 {} / {} 条", saved.records, saved.total);
        });
    }
    Ok(())
}

async fn sync_history_messages(
    app: &tauri::AppHandle,
    login: &QLoginState,
    archive: &ArchiveState,
    session: &Session,
    owner_name: Option<&str>,
) -> Result<std::convert::Infallible, String> {
    let app_copy = app.clone();
    let owner = session.owner.clone();
    let (mut saved, visible_name) = blocking(move || {
        let db = open_database(&app_copy)?;
        Ok((
            checkpoint::read(&db, &owner, "history")?,
            checkpoint::read(&db, &owner, "visible")?.owner_name,
        ))
    })
    .await?;
    set_progress(archive, |p| {
        p.stage = "history";
        p.message = "正在同步历史互动与已删除说说残留…".into();
    });
    let owner_name = owner_name
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .or(saved.owner_name.clone())
        .or(login.nickname().await)
        .or(visible_name);
    saved.owner_name = owner_name.clone();
    // The sparse HTML endpoint has no verified end-of-history marker. Empty
    // windows cannot establish completeness; continue until explicitly paused.
    loop {
        session.check()?;
        let offset = saved.offset;
        // Do not amplify an outage by retrying the same request with a different size.
        let page = session
            .network(qzone::fetch_history_messages(
                login,
                offset,
                30,
                owner_name.as_deref(),
                Some(session),
            ))
            .await?;
        saved.pages += 1;
        saved.records += page.record_count as u64;
        saved.offset = offset
            .checked_add(30)
            .ok_or("历史偏移超出整数表示范围，断点已保留")?;
        saved.complete = false;
        let previous = saved.saved;
        let records = page.record_count as u64;
        saved = checkpoint::save(app, &session.owner, "history", page.feeds, saved).await?;
        set_progress(archive, |p| {
            p.pages += 1;
            p.fetched += records;
            p.saved += saved.saved - previous;
            p.message = format!(
                "已读取 {} 条历史消息，扫描至位置 {}；可随时暂停并继续",
                saved.records, saved.offset
            );
        });
    }
}

#[tauri::command]
pub async fn start_feed_archive(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    archive: tauri::State<'_, ArchiveState>,
    interval_ms: u64,
    mode: Option<ArchiveMode>,
) -> Result<ArchiveProgress, String> {
    let permit = archive
        .task
        .clone()
        .try_lock_owned()
        .map_err(|_| "已有归档或重试任务正在运行")?;
    let owner = login.qzone_auth().await?.uin;
    archive.control.send_replace(Control::Run);
    let session = Session::new(
        app.clone(),
        owner.clone(),
        archive.control.subscribe(),
        archive.requests.clone(),
        interval_ms,
    );
    let mode = mode.unwrap_or_default();
    let initial = ArchiveProgress {
        owner_uin: Some(owner),
        mode,
        status: "running",
        active: true,
        message: "正在恢复归档进度…".into(),
        ..Default::default()
    };
    *archive.progress.lock().map_err(|_| "归档状态锁已损坏")? = initial.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<ArchiveState>();
        let result = task::run_feed_archive(
            app.clone(),
            app.state::<QLoginState>(),
            state.clone(),
            session,
            mode,
        )
        .await;
        if let Err(error) = result {
            set_progress(&state, |p| {
                p.status = "error";
                p.message = format!("归档任务停止：{}", concise_archive_error(&error));
            });
        }
        set_progress(&state, |p| p.active = false);
        drop(permit);
    });
    Ok(initial)
}

#[tauri::command]
pub async fn get_archive_progress(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    state: tauri::State<'_, ArchiveState>,
) -> Result<ArchiveProgress, String> {
    let owner = login.qzone_auth().await?.uin;
    let mut progress = state
        .progress
        .lock()
        .map_err(|_| "归档状态锁已损坏")?
        .clone();
    if progress.owner_uin.as_deref() != Some(&owner) {
        progress = ArchiveProgress {
            owner_uin: Some(owner.clone()),
            ..Default::default()
        };
    }
    if progress.active {
        match *state.control.borrow() {
            Control::Pause => {
                progress.status = "pausing";
                progress.message = "正在暂停并提交已取得的数据…".into();
            }
            Control::Abandon => {
                progress.status = "abandoning";
                progress.message = "正在结束本次任务…".into();
            }
            Control::Run => {}
        }
    }
    blocking(move || {
        let db = open_database(&app)?;
        progress.archived = db
            .query_row(
                &format!("SELECT COUNT(*) FROM archive_dynamics d WHERE d.owner_uin=?1 AND {OWN_MOOD_PREDICATE}"),
                [&owner],
                |r| r.get(0),
            )
            .map_err(|e| format!("统计归档失败：{e}"))?;
        progress.skipped = if progress.mode == ArchiveMode::Recovery { read_unresolved_skip_count(&db, &owner)? } else { 0 };
        if !progress.active {
            progress.complete = progress.mode == ArchiveMode::Own && load_archive_completion(&db, &owner)?;
            let (pages, records, saved, can_resume) = match progress.mode {
                ArchiveMode::Own => {
                    let visible = checkpoint::read(&db, &owner, "visible")?;
                    (visible.pages, visible.records, visible.saved,
                        visible.pages > 0 && !visible.complete)
                }
                ArchiveMode::Recovery => {
                    let history = checkpoint::read(&db, &owner, "history")?;
                    (history.pages, history.records, history.saved, history.pages > 0)
                }
            };
            progress.can_resume = can_resume;
            if progress.pages == 0 {
                progress.pages = pages;
                progress.fetched = records;
                progress.saved = saved;
            }
            let cooldown: i64 = db
                .query_row(
                    "SELECT COALESCE(MAX(retry_at),0) FROM (
                       SELECT retry_at FROM archive_cooldowns WHERE owner_uin=?1
                       UNION ALL SELECT window_started_at+?2 FROM archive_rate_limits
                         WHERE owner_uin=?1 AND requested_pages>=?3)",
                    params![owner, ARCHIVE_RATE_WINDOW_SECONDS, ARCHIVE_RATE_PAGE_LIMIT],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if cooldown > now() {
                progress.status = "limited";
                progress.retry_at = Some(cooldown);
            } else if progress.complete {
                progress.status = "completed";
            } else if progress.status == "idle" && (progress.can_resume || progress.archived > 0) {
                progress.status = "incomplete";
            }
        }
        Ok(progress)
    })
    .await
}

#[tauri::command]
pub async fn list_archive_skips(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
) -> Result<Vec<ArchiveSkipItem>, String> {
    let owner_uin = login.qzone_auth().await?.uin;
    blocking(move || {
        let connection = open_database(&app)?;
        let mut statement = connection
            .prepare(
                "SELECT id,page_number,cursor_offset,offset_advance,base_time,error,skipped_at,
                    retry_count,last_retry_at,resolved_at,recovered_records
             FROM archive_skips WHERE owner_uin=?1
             ORDER BY resolved_at IS NOT NULL, skipped_at DESC",
            )
            .map_err(|error| format!("读取异常跳过列表失败：{error}"))?;
        let rows = statement
            .query_map(params![owner_uin], |row| {
                Ok(ArchiveSkipItem {
                    id: row.get(0)?,
                    page_number: row.get(1)?,
                    cursor_offset: row.get(2)?,
                    offset_advance: row.get(3)?,
                    base_time: row.get(4)?,
                    error: row.get(5)?,
                    skipped_at: row.get(6)?,
                    retry_count: row.get(7)?,
                    last_retry_at: row.get(8)?,
                    resolved_at: row.get(9)?,
                    recovered_records: row.get(10)?,
                })
            })
            .map_err(|error| format!("查询异常跳过列表失败：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("解析异常跳过列表失败：{error}"))
    })
    .await
}

#[tauri::command]
pub async fn retry_archive_skip(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    archive: tauri::State<'_, ArchiveState>,
    id: i64,
) -> Result<ArchiveSkipRetryResult, String> {
    let _permit = archive
        .task
        .clone()
        .try_lock_owned()
        .map_err(|_| "请先暂停归档，再单独重试")?;
    archive.control.send_replace(Control::Run);
    let owner_uin = login.qzone_auth().await?.uin;
    let db_app = app.clone();
    let db_owner = owner_uin.clone();
    let (cursor, resolved_at) = blocking(move || {
        open_database(&db_app)?
            .query_row(
                "SELECT cursor,resolved_at FROM archive_skips WHERE id=?1 AND owner_uin=?2",
                params![id, db_owner],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => "找不到这条异常跳过记录".into(),
                _ => format!("读取异常跳过记录失败：{error}"),
            })
    })
    .await?;
    if resolved_at.is_some() {
        return Ok(ArchiveSkipRetryResult {
            success: true,
            message: "该异常位置已经重试成功".into(),
            recovered_records: 0,
        });
    }
    let session = Session::new(
        app.clone(),
        owner_uin.clone(),
        archive.control.subscribe(),
        archive.requests.clone(),
        3_000,
    );
    let attempted_at = now();
    match session
        .network(qzone::fetch_feeds(
            &login,
            "2",
            Some(&cursor),
            Some(&session),
        ))
        .await
    {
        Ok(page) => {
            let recovered_records = page.feeds.len() as u64;
            let remaining = blocking(move || {
                save_retried_page(
                    &mut open_database(&app)?,
                    &owner_uin,
                    id,
                    &page.feeds,
                    attempted_at,
                )
            })
            .await?;
            set_progress(&archive, |progress| progress.skipped = remaining);
            Ok(ArchiveSkipRetryResult {
                success: true,
                message: format!("重试成功，已恢复 {recovered_records} 条接口记录"),
                recovered_records,
            })
        }
        Err(error) => {
            let summary = concise_archive_error(&error);
            let db_summary = summary.clone();
            blocking(move || {
                open_database(&app)?
                .execute(
                    "UPDATE archive_skips SET retry_count=retry_count+1,last_retry_at=?2,error=?3
                 WHERE id=?1 AND owner_uin=?4",
                    params![id, attempted_at, db_summary, owner_uin],
                )
                .map_err(|reason| format!("保存异常重试失败结果失败：{reason}"))?;
                Ok(())
            })
            .await?;
            Ok(ArchiveSkipRetryResult {
                success: false,
                message: format!("重试仍然失败：{summary}"),
                recovered_records: 0,
            })
        }
    }
}

#[tauri::command]
pub fn cancel_feed_archive(
    state: tauri::State<'_, ArchiveState>,
    task_id: Option<String>,
) -> Result<(), String> {
    let progress = state.progress.lock().map_err(|_| "归档状态锁已损坏")?;
    if task_id.is_some() && task_id != progress.task_id {
        return Err("该同步任务已结束或不属于当前页面".into());
    }
    state.control.send_replace(Control::Pause);
    Ok(())
}

#[tauri::command]
pub async fn abandon_feed_archive(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    state: tauri::State<'_, ArchiveState>,
) -> Result<(), String> {
    let owner = login.qzone_auth().await?.uin;
    let _permit = match state.task.clone().try_lock_owned() {
        Ok(permit) => permit,
        Err(_) => {
            if state
                .progress
                .lock()
                .map_err(|_| "归档状态锁已损坏")?
                .owner_uin
                .as_deref()
                != Some(&owner)
            {
                return Err("请先切回正在归档的账号".into());
            }
            state.control.send_replace(Control::Abandon);
            return Ok(());
        }
    };
    blocking(move || {
        let mut db = open_database(&app)?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        checkpoint::clear(&tx, &owner)?;
        tx.commit().map_err(|e| e.to_string())
    })
    .await?;
    set_progress(&state, |p| {
        p.status = "abandoned";
        p.can_resume = false;
        p.message = "已放弃本次任务，已保存的归档内容保留".into();
    });
    Ok(())
}

fn archived_feeds_query(descending: bool) -> String {
    let (order, comparison) = if descending {
        ("DESC", "<")
    } else {
        ("ASC", ">")
    };
    format!(
            "SELECT d.id,d.owner_uin,d.cell_id,d.published_at,d.content,d.author_uin,d.author_name,d.pictures_json,d.video_json
             FROM archive_dynamics d WHERE d.owner_uin=?1
               AND ?2='self' AND {OWN_MOOD_PREDICATE}
               AND (?3 IS NULL OR CAST(strftime('%Y',d.published_at,'unixepoch','localtime') AS INTEGER)=?3)
               AND (?4 IS NULL
                 OR instr(lower(COALESCE(d.content,'')),lower(?4))>0
                 OR instr(lower(COALESCE(d.author_name,'')),lower(?4))>0
                 OR instr(COALESCE(d.author_uin,''),?4)>0
                 OR EXISTS(SELECT 1 FROM archive_feeds sf
                   WHERE sf.owner_uin=d.owner_uin AND sf.cell_id=d.cell_id
                     AND (instr(lower(COALESCE(sf.event_summary,'')),lower(?4))>0
                       OR instr(lower(COALESCE(sf.actor_name,'')),lower(?4))>0
                       OR instr(COALESCE(sf.actor_uin,''),?4)>0)))
               AND (?7 IS NULL OR d.published_at {comparison} ?7
                 OR (d.published_at=?7 AND d.id {comparison} ?8))
             ORDER BY d.published_at {order},d.id {order} LIMIT ?5 OFFSET ?6"
        )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveQuery {
    limit: u32,
    offset: u32,
    category: String,
    year: Option<i32>,
    descending: Option<bool>,
    query: Option<String>,
    before_time: Option<i64>,
    before_id: Option<i64>,
}

#[tauri::command]
pub async fn list_archived_feeds(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    request: ArchiveQuery,
) -> Result<Vec<ArchiveItem>, String> {
    let ArchiveQuery {
        limit,
        offset,
        category,
        year,
        descending,
        query,
        before_time,
        before_id,
    } = request;
    validate_category(&category)?;
    let owner_uin = login.qzone_auth().await?.uin;
    tauri::async_runtime::spawn_blocking(move || {
        let connection = open_database(&app)?;
        let query = query
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let sql = archived_feeds_query(descending.unwrap_or(true));
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("读取归档失败：{error}"))?;
        let rows = statement
            .query_map(
                params![
                    owner_uin,
                    category,
                    year,
                    query,
                    limit.clamp(1, 200),
                    offset,
                    before_time,
                    before_id
                ],
                archived_item_from_row,
            )
            .map_err(|error| format!("查询归档失败：{error}"))?;
        let mut items = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取归档记录失败：{error}"))?;
        drop(statement);
        hydrate_archive_item_interactions(&connection, &mut items)?;
        Ok(items)
    })
    .await
    .map_err(|error| format!("归档查询任务异常退出：{error}"))?
}

fn archived_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveItem> {
    let urls = video_urls(row.get(8)?);
    Ok(ArchiveItem {
        id: row.get(0)?,
        owner_uin: row.get(1)?,
        cell_id: row.get(2)?,
        published_at: row.get(3)?,
        content: row.get(4)?,
        author_uin: row.get(5)?,
        author_name: row.get(6)?,
        picture_urls: picture_urls(row.get(7)?),
        video_urls: urls,
        like_count: 0,
        comment_count: 0,
        likes: vec![],
        comments: vec![],
    })
}

fn hydrate_archive_item_interactions(
    connection: &Connection,
    items: &mut [ArchiveItem],
) -> Result<(), String> {
    let mut comment_statement = connection
        .prepare_cached(
            "SELECT comments_json,actor_uin,actor_name,event_summary,event_time FROM archive_feeds
             WHERE owner_uin=?1 AND cell_id=?2 AND event_type IN (2,311) ORDER BY event_time ASC",
        )
        .map_err(|error| format!("准备评论查询失败：{error}"))?;
    for item in items.iter_mut() {
        let comments = comment_statement
            .query_map(params![item.owner_uin, item.cell_id], |row| {
                Ok(comment_from_values(
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|error| format!("查询动态评论失败：{error}"))?;
        item.comments = merge_comments(
            comments
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("读取评论失败：{e}"))?,
        );
        item.comment_count = comment_interaction_count(&item.comments);
    }
    drop(comment_statement);
    let mut like_statement = connection
        .prepare_cached(
            "SELECT actor_uin,actor_name,
                    (feed_key LIKE 'history-v2-event:%'),event_time
             FROM archive_feeds
             WHERE owner_uin=?1 AND cell_id=?2 AND event_type=217
             ORDER BY (feed_key LIKE 'history-v2-event:%') ASC,event_time ASC",
        )
        .map_err(|error| format!("准备点赞查询失败：{error}"))?;
    for item in items.iter_mut() {
        let likes = like_statement
            .query_map(params![item.owner_uin, item.cell_id], |row| {
                Ok(LikeUser {
                    uin: row.get(0)?,
                    nickname: row.get(1)?,
                    historical: row.get(2)?,
                    liked_at: row.get(3)?,
                })
            })
            .map_err(|error| format!("查询点赞用户失败：{error}"))?;
        item.likes = deduplicate_likes(
            likes
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("读取点赞失败：{e}"))?,
        );
        item.like_count = item.likes.len() as i64;
    }
    Ok(())
}

fn comment_from_values(
    json: Option<String>,
    fallback_uin: Option<String>,
    fallback_name: Option<String>,
    fallback_content: Option<String>,
    fallback_time: i64,
) -> ArchiveComment {
    let value = json.and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let main = value.as_ref().and_then(|value| value.get("main_comment"));
    let comment_id = main.and_then(|value| text_at(value, "/commentid"));
    let main_uin = main.and_then(|value| text_at(value, "/user/uin"));
    let main_name = main.and_then(|value| text_at(value, "/user/nickname"));
    let main_content = main.and_then(|value| text_at(value, "/content"));
    let main_time = main
        .and_then(|value| value.get("date"))
        .and_then(Value::as_i64)
        .unwrap_or(fallback_time);
    let mut replies: Vec<ArchiveReply> = main
        .and_then(|value| value.get("replys"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(reply_from_value)
        .collect();
    if let Some(comment_id) = comment_id.as_deref() {
        let related_replies = value
            .as_ref()
            .and_then(|value| value.get("comments"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|comment| text_at(comment, "/commentid").as_deref() == Some(comment_id))
            .filter_map(|comment| comment.get("replys"))
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(reply_from_value);
        for reply in related_replies {
            let duplicate = replies.iter().any(|candidate| {
                candidate.uin == reply.uin
                    && candidate.content == reply.content
                    && candidate.created_at == reply.created_at
            });
            if !duplicate {
                replies.push(reply);
            }
        }
    }

    // Reply notifications keep the parent in main_comment but put the actual
    // reply text and author at the feed level. When the parent author replies
    // again, target the latest preceding reply from the other participant.
    let is_reply_notification = main
        .and_then(|value| value.get("replynum"))
        .and_then(Value::as_i64)
        .is_some_and(|count| count > 0)
        && main_uin.is_some()
        && fallback_uin.is_some()
        && main_content.as_deref() != fallback_content.as_deref()
        && fallback_time > main_time;
    if is_reply_notification {
        if let Some(content) = fallback_content.clone() {
            let duplicate = replies
                .iter()
                .any(|reply| reply.uin == fallback_uin && reply.content == content);
            if !duplicate {
                let reply_target = replies
                    .iter()
                    .filter(|reply| reply.uin != fallback_uin && reply.created_at <= fallback_time)
                    .max_by_key(|reply| reply.created_at);
                replies.push(ArchiveReply {
                    uin: fallback_uin.clone(),
                    nickname: fallback_name.clone(),
                    reply_to_uin: reply_target
                        .and_then(|reply| reply.uin.clone())
                        .or_else(|| main_uin.clone()),
                    reply_to_nickname: reply_target
                        .and_then(|reply| reply.nickname.clone())
                        .or_else(|| main_name.clone()),
                    content,
                    created_at: fallback_time,
                });
            }
        }
    }

    let preferred_name = main_name
        .clone()
        .filter(|name| {
            !name.trim().is_empty() && main_uin.as_deref().is_none_or(|uin| name.trim() != uin)
        })
        .or_else(|| {
            fallback_name.clone().filter(|name| {
                !name.trim().is_empty()
                    && fallback_uin.as_deref().is_none_or(|uin| name.trim() != uin)
            })
        })
        .or(main_name)
        .or(fallback_name);
    ArchiveComment {
        comment_id,
        uin: main_uin.or(fallback_uin),
        nickname: preferred_name,
        content: main_content
            .or(fallback_content)
            .unwrap_or_else(|| "评论了这条动态".into()),
        created_at: main_time,
        replies,
    }
}

fn reply_from_value(value: &Value) -> Option<ArchiveReply> {
    let content = text_at(value, "/content")?;
    Some(ArchiveReply {
        uin: text_at(value, "/user/uin").or_else(|| text_at(value, "/replyuser/uin")),
        nickname: text_at(value, "/user/nickname")
            .or_else(|| text_at(value, "/replyuser/nickname")),
        reply_to_uin: text_at(value, "/replyuser/uin")
            .or_else(|| text_at(value, "/targetuser/uin"))
            .or_else(|| text_at(value, "/target/uin")),
        reply_to_nickname: text_at(value, "/replyuser/nickname")
            .or_else(|| text_at(value, "/targetuser/nickname"))
            .or_else(|| text_at(value, "/target/nickname")),
        content,
        created_at: value.get("date").and_then(Value::as_i64).unwrap_or(0),
    })
}

fn merge_comments(comments: impl IntoIterator<Item = ArchiveComment>) -> Vec<ArchiveComment> {
    let mut merged: Vec<ArchiveComment> = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut signatures: HashMap<(Option<String>, String, i64), usize> = HashMap::new();
    let mut reply_keys: Vec<HashSet<(Option<String>, String, i64)>> = Vec::new();
    for mut comment in comments {
        let signature = (
            comment.uin.clone(),
            comment.content.clone(),
            comment.created_at,
        );
        // Preserve first-match semantics when ID and content point to different rows.
        let existing = comment
            .comment_id
            .as_ref()
            .and_then(|id| ids.get(id))
            .into_iter()
            .chain(signatures.get(&signature))
            .copied()
            .min();
        if let Some(index) = existing {
            for reply in comment.replies.drain(..) {
                if reply_keys[index].insert((
                    reply.uin.clone(),
                    reply.content.clone(),
                    reply.created_at,
                )) {
                    merged[index].replies.push(reply);
                }
            }
        } else {
            let index = merged.len();
            if let Some(id) = &comment.comment_id {
                ids.entry(id.clone()).or_insert(index);
            }
            signatures.entry(signature).or_insert(index);
            reply_keys.push(
                comment
                    .replies
                    .iter()
                    .map(|r| (r.uin.clone(), r.content.clone(), r.created_at))
                    .collect(),
            );
            merged.push(comment);
        }
    }
    for comment in &mut merged {
        comment.replies.sort_by_key(|reply| reply.created_at);
    }
    merged
}

fn comment_interaction_count(comments: &[ArchiveComment]) -> i64 {
    comments
        .iter()
        .map(|comment| 1_i64.saturating_add(comment.replies.len() as i64))
        .sum()
}

fn deduplicate_likes(likes: impl IntoIterator<Item = LikeUser>) -> Vec<LikeUser> {
    let mut seen = HashSet::new();
    likes
        .into_iter()
        .filter(|like| {
            let uin = like.uin.as_deref().unwrap_or("").trim();
            let nickname = like.nickname.as_deref().unwrap_or("").trim();
            // A person may have changed nicknames between the legacy history
            // endpoint and the current feed endpoint. QQ number is the stable
            // identity; only fall back to the nickname when no UIN survived.
            seen.insert(if uin.is_empty() {
                format!("name:{nickname}")
            } else {
                format!("uin:{uin}")
            })
        })
        .collect()
}

fn validate_category(category: &str) -> Result<(), String> {
    match category {
        "self" => Ok(()),
        _ => Err("无效的归档分类".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{comment_from_values, merge_comments, parse_feed, video_cover_url, video_urls};
    use rusqlite::{params, Connection};
    use serde_json::json;
    use std::fs;

    #[test]
    fn library_pagination_never_ends_on_short_pages_or_reported_count() {
        assert!(!super::library::library_page_complete(1, 1, Some(100)).unwrap());
        assert!(!super::library::library_page_complete(100001, 100, Some(100)).unwrap());
        assert!(!super::library::library_page_complete(100001, 1, None).unwrap());
        assert!(super::library::library_page_complete(100001, 0, Some(100)).unwrap());
        assert!(super::library::library_page_complete(20, 0, Some(21)).is_err());
        assert!(super::library::parsed_library_page("photos", json!({"data":{}})).is_err());
        assert!(super::library::parsed_library_page(
            "photos",
            json!({"data":{"photoList":[{"id":"a"}]}})
        )
        .unwrap()
        .total
        .is_none());
    }

    #[test]
    fn library_records_roll_back_with_cursor_and_preserve_equal_content_identities() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        let items = vec![
            json!({"id":"a","name":"相同内容"}),
            json!({"id":"b","name":"相同内容"}),
        ];
        db.execute_batch("CREATE TRIGGER reject_cursor BEFORE INSERT ON archive_library_state BEGIN SELECT RAISE(ABORT,'cursor failed'); END;").unwrap();
        {
            let tx = db.transaction().unwrap();
            super::library::save_library_page(&tx, "100", "albums", "", "", &items).unwrap();
            assert!(
                super::library::save_library_state(&tx, "100", "albums", "", 2, 2, false).is_err()
            );
        }
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM archive_library_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute("DROP TRIGGER reject_cursor", []).unwrap();
        let tx = db.transaction().unwrap();
        super::library::save_library_page(&tx, "100", "albums", "", "", &items).unwrap();
        super::library::save_library_state(&tx, "100", "albums", "", 2, 2, false).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM archive_library_items WHERE owner_uin='100'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT fetched FROM archive_library_state WHERE owner_uin='100'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
    }

    #[test]
    fn image_cache_rejects_non_image_content_and_known_missing_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("photo.jpg"),
            b"<html>This is an error response, not a picture</html>",
        )
        .unwrap();
        assert!(super::existing_archived_image(dir.path(), "photo").is_none());
        let mut placeholder = vec![0; 2038];
        placeholder[..6].copy_from_slice(b"GIF89a");
        placeholder[6..8].copy_from_slice(&340u16.to_le_bytes());
        placeholder[8..10].copy_from_slice(&320u16.to_le_bytes());
        fs::write(dir.path().join("photo.gif"), placeholder).unwrap();
        assert!(super::existing_archived_image(dir.path(), "photo").is_none());
        let valid = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
        )
        .unwrap();
        fs::write(dir.path().join("photo.gif"), valid).unwrap();
        assert_eq!(
            super::existing_archived_image(dir.path(), "photo"),
            Some(dir.path().join("photo.gif"))
        );
    }

    #[test]
    fn archive_only_stores_owned_moods_and_their_interactions() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        let body = |id: &str, appid: u32, author: &str| {
            json!({
                "comm":{"feedskey":format!("body-{id}"),"subid":0,"time":100},
                "original":{"cell_id":{"cellid":id},"cell_comm":{"appid":appid},
                    "cell_userinfo":{"user":{"uin":author}},"cell_summary":{"summary":"正文"}}
            })
        };
        let reference = |id: &str, key: &str| {
            json!({
                "comm":{"feedskey":key,"subid":217,"time":101},
                "original":{"cell_id":{"cellid":id}},
                "userinfo":{"user":{"uin":"200","nickname":"好友"}}
            })
        };
        let tx = db.transaction().unwrap();
        // A reference preceding its post must survive; unrelated/orphan records must not.
        assert_eq!(
            super::save_feed_rows(
                &tx,
                "100",
                &[
                    reference("mood", "like"),
                    reference("unknown", "orphan"),
                    body("mood", 311, "100"),
                    body("other-app-4", 4, "100"),
                    body("other-app-202", 202, "100"),
                    body("guestbook", 334, "100"),
                    body("friend", 311, "200"),
                    body("history-v3:fragment", 311, "100"),
                ]
            )
            .unwrap(),
            2
        );
        // An explicit conflicting identity cannot overwrite a verified mood.
        assert_eq!(
            super::save_feed_rows(&tx, "100", &[body("mood", 4, "100")]).unwrap(),
            0
        );
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM archive_dynamics", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM archive_feeds", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        // Pre-existing non-mood data is preserved, but excluded from counts and lists.
        db.execute("INSERT INTO archive_dynamics(owner_uin,author_uin,cell_id,category,raw_original_json,archived_at)
            VALUES ('100','100','old-other','self','{\"cell_comm\":{\"appid\":202}}',1)", []).unwrap();
        assert_eq!(
            db.query_row(
                &format!(
                    "SELECT COUNT(*) FROM archive_dynamics d WHERE {}",
                    super::OWN_MOOD_PREDICATE
                ),
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        let ids = db
            .prepare(&super::archived_feeds_query(true))
            .unwrap()
            .query_map(
                params![
                    "100",
                    "self",
                    Option::<i32>::None,
                    Option::<String>::None,
                    20,
                    0,
                    Option::<i64>::None,
                    Option::<i64>::None
                ],
                |r| r.get::<_, String>(2),
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids, ["mood"]);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM archive_dynamics", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn sparse_history_and_references_cannot_erase_post_media() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        let full = json!({"original":{"cell_id":{"cellid":"post"},"cell_comm":{"appid":311,"time":100},
            "cell_summary":{"summary":"完整正文"},"cell_userinfo":{"user":{"uin":"100","nickname":"本人"}},
            "cell_pic":{"picdata":{"pic":(0..64).map(|i| json!({"photourl":[{"url":format!("https://example.com/{i}.jpg")}]})).collect::<Vec<_>>()}},
            "cell_video":{"videourls":[{"url":"https://example.com/video.mp4"}]}}});
        let sparse = json!({"original":{"cell_id":{"cellid":"post"},"cell_comm":{"time":200},
            "cell_summary":{"summary":""},"cell_pic":{"picdata":{"pic":[{"photourl":[{"url":"https://example.com/0.jpg"},{"url":"https://example.com/0-original.jpg"}]}]}},
            "cell_video":{}}});
        let tx = db.transaction().unwrap();
        super::save_original_dynamic(&tx, "100", &full).unwrap();
        super::save_original_dynamic(&tx, "100", &sparse).unwrap();
        super::save_original_dynamic(
            &tx,
            "100",
            &json!({"original":{"cell_id":{"cellid":"post"}}}),
        )
        .unwrap();
        tx.commit().unwrap();
        let (pictures, video, content, category): (Option<String>, Option<String>, String, String) =
            db.query_row(
                "SELECT pictures_json,video_json,content,category FROM archive_dynamics",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        let pictures = super::picture_url_candidates(pictures);
        assert_eq!(pictures.len(), 64);
        assert_eq!(pictures[0].len(), 2);
        assert_eq!(pictures[63][0], "https://example.com/63.jpg");
        assert_eq!(super::video_urls(video), ["https://example.com/video.mp4"]);
        assert_eq!(content, "完整正文");
        assert_eq!(category, "self");
        assert_eq!(
            db.query_row(
                &format!(
                    "SELECT COUNT(*) FROM archive_dynamics d WHERE {}",
                    super::OWN_MOOD_PREDICATE
                ),
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn initializes_current_schema_and_preserves_data_and_settings_on_reopen() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        db.execute("INSERT INTO archive_dynamics(owner_uin,cell_id,category,raw_original_json,archived_at) VALUES ('100','post','self','{}',1)", []).unwrap();
        db.execute(
            "INSERT INTO archive_stage_checkpoints VALUES ('100','history',?1)",
            [serde_json::to_string(&super::checkpoint::StageCheckpoint {
                offset: 12345,
                ..Default::default()
            })
            .unwrap()],
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='archive_migrations'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        // Opening a ready database needs no DDL or data writes.
        db.pragma_update(None, "foreign_keys", false).unwrap();
        db.pragma_update(None, "query_only", true).unwrap();
        super::initialize_database(&mut db).unwrap();
        assert!(db
            .pragma_query_value(None, "foreign_keys", |r| r.get::<_, bool>(0))
            .unwrap());
        assert_eq!(
            db.query_row("SELECT cell_id,category FROM archive_dynamics", [], |r| Ok(
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?)
            ))
            .unwrap(),
            ("post".into(), "self".into())
        );
        assert_eq!(
            super::checkpoint::read(&db, "100", "history")
                .unwrap()
                .offset,
            12345
        );
    }

    #[test]
    fn failed_schema_initialization_rolls_back_tables_and_marker() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute("CREATE TABLE archive_dynamics(id INTEGER)", [])
            .unwrap();
        assert!(super::initialize_database(&mut db).is_err());
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='archive_feeds'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        db.execute("DROP TABLE archive_dynamics", []).unwrap();
        super::initialize_database(&mut db).unwrap();
    }

    #[test]
    fn finalization_rolls_back_completion_when_checkpoint_cleanup_fails() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        db.execute("INSERT INTO archive_stage_checkpoints VALUES ('100','history','{}'),('200','history','{}')", []).unwrap();
        db.execute_batch(
            "CREATE TRIGGER fail_cleanup BEFORE DELETE ON archive_stage_checkpoints
            BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;",
        )
        .unwrap();
        assert!(super::finalize_archive(&mut db, "100", true, true).is_err());
        assert!(!super::load_archive_completion(&db, "100").unwrap());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM archive_stage_checkpoints", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            2
        );
        db.execute("DROP TRIGGER fail_cleanup", []).unwrap();
        super::finalize_archive(&mut db, "100", true, true).unwrap();
        assert!(super::load_archive_completion(&db, "100").unwrap());
        assert_eq!(
            db.query_row("SELECT owner_uin FROM archive_stage_checkpoints", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "200"
        );
    }

    #[test]
    fn indexed_comment_merge_preserves_first_match_when_id_and_content_disagree() {
        let make = |id: &str, uin: &str, content: &str, time| super::ArchiveComment {
            comment_id: Some(id.into()),
            uin: Some(uin.into()),
            nickname: None,
            content: content.into(),
            created_at: time,
            replies: vec![],
        };
        let mut incoming = make("B", "1", "first", 10);
        incoming.replies.push(super::ArchiveReply {
            uin: Some("3".into()),
            nickname: None,
            reply_to_uin: None,
            reply_to_nickname: None,
            content: "reply".into(),
            created_at: 30,
        });
        let merged = merge_comments([
            make("A", "1", "first", 10),
            make("B", "2", "second", 20),
            incoming,
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].comment_id.as_deref(), Some("A"));
        assert_eq!(merged[0].replies.len(), 1);
        assert!(merged[1].replies.is_empty());
    }

    #[test]
    #[ignore = "offline comment merge benchmark; no network requests"]
    fn comment_merge_benchmark() {
        let input = (0..2)
            .flat_map(|round| {
                (0..10_000).map(move |i| super::ArchiveComment {
                    comment_id: Some(format!("id-{i}")),
                    uin: Some("100".into()),
                    nickname: None,
                    content: format!("comment {i}"),
                    created_at: i,
                    replies: vec![super::ArchiveReply {
                        uin: Some("200".into()),
                        nickname: None,
                        reply_to_uin: None,
                        reply_to_nickname: None,
                        content: format!("round-{round}"),
                        created_at: round,
                    }],
                })
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let result = merge_comments(input);
        let elapsed = started.elapsed();
        assert_eq!(result.len(), 10_000);
        assert_eq!(
            result.iter().map(|c| c.replies.len()).sum::<usize>(),
            20_000
        );
        for (i, comment) in result.iter().enumerate() {
            assert_eq!(comment.created_at, i as i64);
            assert_eq!(comment.replies[0].created_at, 0);
            assert_eq!(comment.replies[1].created_at, 1);
        }
        println!(
            "comments=10000 input=20000 replies=20000 elapsed_ms={:.2}",
            elapsed.as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn ipc_archive_query_decodes_camel_case_cursor_and_optional_filters() {
        let request: super::ArchiveQuery = serde_json::from_value(json!({
            "category":"self", "limit":21, "offset":0, "beforeTime":200, "beforeId":3
        }))
        .unwrap();
        assert_eq!(request.category, "self");
        assert_eq!((request.limit, request.offset), (21, 0));
        assert_eq!(
            (request.before_time, request.before_id),
            (Some(200), Some(3))
        );
        assert!(request.year.is_none() && request.query.is_none() && request.descending.is_none());
        assert!(serde_json::from_value::<super::ArchiveQuery>(json!({"category":"self"})).is_err());
    }

    #[test]
    fn feed_query_paginates_equal_timestamps_and_is_account_scoped() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        for (id, owner, time) in [
            (1, "100", 100),
            (2, "100", 200),
            (3, "100", 200),
            (4, "200", 100),
        ] {
            db.execute("INSERT INTO archive_dynamics(id,owner_uin,author_uin,cell_id,published_at,category,raw_original_json,archived_at)
                VALUES (?1,?2,?2,?1,?3,'self','{\"cell_comm\":{\"appid\":311}}',1)", params![id,owner,time]).unwrap();
        }
        let ids = |descending, time: Option<i64>, id: Option<i64>| {
            db.prepare(&super::archived_feeds_query(descending))
                .unwrap()
                .query_map(
                    params![
                        "100",
                        "self",
                        Option::<i32>::None,
                        Option::<String>::None,
                        20,
                        0,
                        time,
                        id
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(ids(true, None, None), vec![3, 2, 1]);
        assert_eq!(ids(true, Some(200), Some(3)), vec![2, 1]);
        assert_eq!(ids(false, Some(200), Some(2)), vec![3]);
        db.execute("INSERT INTO archive_dynamics(id,owner_uin,author_uin,cell_id,published_at,category,raw_original_json,archived_at)
                VALUES (5,'100','100','new',300,'self','{\"cell_comm\":{\"appid\":311}}',1)", []).unwrap();
        // New arrivals must not shift the next page or duplicate previously displayed rows.
        assert_eq!(ids(true, Some(200), Some(3)), vec![2, 1]);
    }

    #[test]
    fn retry_commits_records_and_resolved_marker_together() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        db.execute("INSERT INTO archive_skips(owner_uin,cursor,resume_cursor,page_number,cursor_offset,offset_advance,base_time,error,skipped_at)
            VALUES ('100','cursor','cursor',1,0,0,1,'timeout',1)", []).unwrap();
        let feeds = vec![
            json!({"comm":{"feedskey":"test-like","subid":217,"time":100},
            "original":{"cell_id":{"cellid":"mood1"},"cell_comm":{"appid":311},"cell_summary":{"summary":"正文"},
              "cell_userinfo":{"user":{"uin":"100","nickname":"本人"}}},
            "userinfo":{"user":{"uin":"200","nickname":"好友"}}}),
        ];
        db.execute_batch("CREATE TRIGGER fail_retry BEFORE UPDATE ON archive_skips BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        assert!(super::save_retried_page(&mut db, "100", 1, &feeds, 2).is_err());
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM archive_feeds", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        db.execute_batch("DROP TRIGGER fail_retry;").unwrap();
        assert_eq!(
            super::save_retried_page(&mut db, "100", 1, &feeds, 2).unwrap(),
            0
        );
        let resolved: i64 = db
            .query_row(
                "SELECT resolved_at FROM archive_skips WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(resolved, 2);
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM archive_feeds", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn initializes_archive_schema_and_enforces_account_request_budgets() {
        let mut db = Connection::open_in_memory().unwrap();
        super::initialize_database(&mut db).unwrap();
        // Reopening an existing database must preserve its contents and checkpoints.
        super::initialize_database(&mut db).unwrap();
        for _ in 0..super::ARCHIVE_RATE_PAGE_LIMIT {
            assert_eq!(super::reserve_request(&mut db, "100", 1_000).unwrap(), None);
        }
        assert_eq!(
            super::reserve_request(&mut db, "100", 1_001).unwrap(),
            Some(1_600)
        );
        assert_eq!(super::reserve_request(&mut db, "200", 1_001).unwrap(), None);
        assert_eq!(super::reserve_request(&mut db, "100", 1_600).unwrap(), None);
        db.execute("INSERT INTO archive_cooldowns VALUES ('100',2000)", [])
            .unwrap();
        assert_eq!(
            super::reserve_request(&mut db, "100", 1_601).unwrap(),
            Some(2_000)
        );
    }

    #[test]
    fn parses_guestbook_wall_clock_time_after_zero_modify_time() {
        let created_at = super::library::library_created_at(&json!({
            "modifytime": 0,
            "pubtime": "2020-08-08 06:40:11"
        }));
        assert_eq!(created_at, 1_596_840_011);
    }

    #[test]
    fn parses_like_event_sample_shape() {
        let feed = json!({"comm":{"feedskey":"217_3_key","subid":217,"time":1752553379},
          "original":{"cell_id":{"cellid":"mood1"},"cell_summary":{"summary":"：纪念"},
          "cell_userinfo":{"user":{"uin":"1","nickname":"主人"}},"cell_video":{"videoid":"v1"}},
          "title":{"title":"赞了我"},"userinfo":{"user":{"uin":"2","nickname":"访客"}}});
        let parsed = parse_feed(&feed).unwrap();
        assert_eq!(parsed.feed_key, "217_3_key");
        assert_eq!(parsed.event_type, 217);
        assert!(parsed.video_json.is_some());
    }

    #[test]
    fn parses_comment_and_picture_sample_shape() {
        let feed = json!({"comm":{"feedskey":"311_2_key","subid":2,"time":1751637966},
          "original":{"cell_id":{"cellid":"mood2"},"cell_summary":{"summary":"：哼哧哼哧"},
          "cell_pic":{"picdata":{"pic":[{},{}]}},"cell_comment":{"main_comment":{"content":"评论"}}},
          "summary":{"summary":"又幸福上了"},"userinfo":{"user":{"uin":"3","nickname":"评论者"}}});
        let parsed = parse_feed(&feed).unwrap();
        assert_eq!(parsed.event_type, 2);
        assert_eq!(parsed.picture_count, 2);
        assert!(parsed.comments_json.is_some());
    }

    #[test]
    fn nests_feed_level_reply_under_its_parent_comment() {
        let comments = json!({
            "main_comment": {
                "content": "给我我好想要",
                "date": 1785795539_i64,
                "replynum": 1,
                "replys": null,
                "user": { "uin": "718038005", "nickname": "此刻春和景明_" }
            }
        });

        let comment = comment_from_values(
            Some(comments.to_string()),
            Some("1027704977".into()),
            Some("轻鹄".into()),
            Some("[em]e10324[/em]".into()),
            1785807754,
        );

        assert_eq!(comment.content, "给我我好想要");
        assert_eq!(comment.replies.len(), 1);
        assert_eq!(comment.replies[0].nickname.as_deref(), Some("轻鹄"));
        assert_eq!(comment.replies[0].content, "[em]e10324[/em]");
        assert_eq!(comment.replies[0].created_at, 1785807754);
    }

    #[test]
    fn does_not_turn_a_regular_comment_into_its_own_reply() {
        let comments = json!({
            "main_comment": {
                "content": "入才",
                "date": 1743068483_i64,
                "replynum": 0,
                "replys": null,
                "user": { "uin": "1027704977", "nickname": "轻鹄" }
            }
        });

        let comment = comment_from_values(
            Some(comments.to_string()),
            Some("1027704977".into()),
            Some("轻鹄".into()),
            Some("入才".into()),
            1743068483,
        );

        assert!(comment.replies.is_empty());
    }

    #[test]
    fn merges_multi_round_replies_and_preserves_each_target() {
        let first = json!({
            "main_comment": {
                "commentid": "parent-1",
                "content": "父评论",
                "date": 100_i64,
                "replynum": 1,
                "replys": [{
                    "content": "第一轮",
                    "date": 110_i64,
                    "user": { "uin": "2", "nickname": "乙" },
                    "replyuser": { "uin": "1", "nickname": "甲" }
                }],
                "user": { "uin": "1", "nickname": "甲" }
            }
        });
        let second = json!({
            "main_comment": {
                "commentid": "parent-1",
                "content": "父评论",
                "date": 100_i64,
                "replynum": 1,
                "replys": [{
                    "content": "第二轮",
                    "date": 120_i64,
                    "user": { "uin": "1", "nickname": "甲" },
                    "replyuser": { "uin": "2", "nickname": "乙" }
                }],
                "user": { "uin": "1", "nickname": "甲" }
            }
        });

        let comments = merge_comments([
            comment_from_values(Some(first.to_string()), None, None, None, 0),
            comment_from_values(Some(second.to_string()), None, None, None, 0),
        ]);

        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].replies.len(), 2);
        assert_eq!(comments[0].replies[0].nickname.as_deref(), Some("乙"));
        assert_eq!(
            comments[0].replies[0].reply_to_nickname.as_deref(),
            Some("甲")
        );
        assert_eq!(comments[0].replies[1].nickname.as_deref(), Some("甲"));
        assert_eq!(
            comments[0].replies[1].reply_to_nickname.as_deref(),
            Some("乙")
        );
        assert_eq!(comments[0].replies[1].created_at, 120);
    }

    #[test]
    fn includes_owner_reply_stored_in_comments_array() {
        let comments = json!({
            "comments": [{
                "commentid": "1",
                "content": "嘎嘎咕咕",
                "date": 1786197473_i64,
                "user": { "uin": "1027704977", "nickname": "轻鹄" },
                "replys": [{
                    "replyid": "1",
                    "content": "咕咕咕咕嘎嘎",
                    "date": 1786197525_i64,
                    "user": { "uin": "718038005", "nickname": "此刻春和景明_" },
                    "target": { "uin": "1027704977", "nickname": "轻鹄" }
                }]
            }],
            "main_comment": {
                "commentid": "1",
                "content": "嘎嘎咕咕",
                "date": 1786197473_i64,
                "replynum": 1,
                "replys": null,
                "user": { "uin": "1027704977", "nickname": "轻鹄" }
            }
        });

        let comment = comment_from_values(
            Some(comments.to_string()),
            Some("1027704977".into()),
            Some("轻鹄".into()),
            Some("嘎嘎咕咕".into()),
            1786197473,
        );

        assert_eq!(comment.content, "嘎嘎咕咕");
        assert_eq!(comment.replies.len(), 1);
        assert_eq!(
            comment.replies[0].nickname.as_deref(),
            Some("此刻春和景明_")
        );
        assert_eq!(comment.replies[0].content, "咕咕咕咕嘎嘎");
        assert_eq!(
            comment.replies[0].reply_to_nickname.as_deref(),
            Some("轻鹄")
        );
        assert_eq!(comment.replies[0].created_at, 1786197525);
    }

    #[test]
    fn attaches_parent_author_follow_up_to_latest_child_reply() {
        let comments = json!({
            "comments": [{
                "commentid": "1",
                "content": "嘎嘎咕咕",
                "date": 1786197473_i64,
                "user": { "uin": "1027704977", "nickname": "轻鹄" },
                "replys": [
                    {
                        "replyid": "2",
                        "content": "咕咕嘎嘎咕咕嘎嘎",
                        "date": 1786199046_i64,
                        "user": { "uin": "718038005", "nickname": "此刻春和景明_" },
                        "target": { "uin": "1027704977", "nickname": "轻鹄" }
                    },
                    {
                        "replyid": "3",
                        "content": "凑凑凑凑凑企鹅",
                        "date": 1786199059_i64,
                        "user": { "uin": "718038005", "nickname": "此刻春和景明_" },
                        "target": { "uin": "1027704977", "nickname": "轻鹄" }
                    }
                ]
            }],
            "main_comment": {
                "commentid": "1",
                "content": "嘎嘎咕咕",
                "date": 1786197473_i64,
                "replynum": 4,
                "replys": null,
                "user": { "uin": "1027704977", "nickname": "轻鹄" }
            }
        });

        let comment = comment_from_values(
            Some(comments.to_string()),
            Some("1027704977".into()),
            Some("轻鹄".into()),
            Some("人才咕嘎咕嘎".into()),
            1786199104,
        );

        assert_eq!(comment.replies.len(), 3);
        let follow_up = &comment.replies[2];
        assert_eq!(follow_up.nickname.as_deref(), Some("轻鹄"));
        assert_eq!(follow_up.content, "人才咕嘎咕嘎");
        assert_eq!(
            follow_up.reply_to_nickname.as_deref(),
            Some("此刻春和景明_")
        );
        assert_eq!(follow_up.created_at, 1786199104);
    }

    #[test]
    fn creates_stable_key_for_feed_without_server_identifiers() {
        let feed = json!({
          "comm":{"subid":999,"time":1751637966},
          "summary":{"summary":"一种没有 feedskey 和 cell_id 的特殊互动"},
          "userinfo":{"user":{"uin":"3","nickname":"互动用户"}}
        });

        let first = parse_feed(&feed).expect("特殊互动不应中断整页归档");
        let second = parse_feed(&feed).expect("同一互动应当能重复解析");

        assert!(first.feed_key.starts_with("fallback:999:1751637966:3:"));
        assert_eq!(first.feed_key, second.feed_key);
    }

    #[test]
    fn reads_video_fallbacks_from_legacy_and_normalized_shapes() {
        let value = json!({
            "videourl": "https://example.com/high.mp4",
            "actionurl": "https://example.com/action.mp4",
            "videourls": {
                "5": {"url": "https://example.com/low.mp4"},
                "9": {"url": "https://example.com/high.mp4"}
            },
            "coverurl": "https://example.com/cover.jpg"
        });
        assert_eq!(
            video_urls(Some(value.to_string())),
            vec![
                "https://example.com/high.mp4",
                "https://example.com/action.mp4",
                "https://example.com/low.mp4"
            ]
        );
        assert_eq!(
            video_cover_url(Some(value.to_string())).as_deref(),
            Some("https://example.com/cover.jpg")
        );
    }
}
