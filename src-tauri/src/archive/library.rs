//! Shared resource synchronization, page persistence and read projections.
use super::library_content::{self, LibraryDetails};
use super::runtime::{Control, Session};
use super::{
    blocking, concise_archive_error, gallery, now, open_database, set_progress, stable_feed_hash,
    ArchiveProgress, ArchiveState,
};
use crate::commands::{qlogin::QLoginState, qzone};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    id: i64,
    module: String,
    item_key: String,
    parent_key: String,
    created_at: i64,
    title: String,
    summary: String,
    author_uin: Option<String>,
    author_name: Option<String>,
    cover_url: Option<String>,
    media_urls: Vec<String>,
    #[serde(flatten)]
    details: LibraryDetails,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPage {
    items: Vec<LibraryItem>,
    total: u64,
    remote_total: u64,
    complete: bool,
    synced_at: i64,
    last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySyncResult {
    module: String,
    fetched: u64,
    saved: u64,
    remote_total: u64,
    complete: bool,
    message: String,
}

pub(super) struct ParsedLibraryPage {
    pub(super) items: Vec<Value>,
    pub(super) total: Option<usize>,
}

fn validate_library_module(module: &str) -> Result<(), String> {
    match module {
        "albums" | "photos" | "videos" | "guestbook" | "favorites" => Ok(()),
        _ => Err("无效的资料归档模块".into()),
    }
}

pub(super) fn value_text(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let entry = value.get(*name)?;
        let text = entry
            .as_str()
            .map(str::to_owned)
            .or_else(|| entry.as_i64().map(|number| number.to_string()))
            .or_else(|| entry.as_u64().map(|number| number.to_string()))?;
        (!text.trim().is_empty()).then(|| text.trim().to_owned())
    })
}

pub(super) fn library_created_at(value: &Value) -> i64 {
    for name in [
        "uploadtime",
        "uploadTime",
        "createtime",
        "create_time",
        "pubtime",
        "time",
        "modifytime",
    ] {
        let Some(entry) = value.get(name) else {
            continue;
        };
        let numeric = entry
            .as_i64()
            .or_else(|| entry.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| {
                entry
                    .as_str()
                    .and_then(|text| text.trim().parse::<i64>().ok())
            })
            .unwrap_or(0);
        if numeric > 0 {
            return if numeric > 10_000_000_000 {
                numeric / 1_000
            } else {
                numeric
            };
        }
        if let Some(text) = entry.as_str() {
            let parsed = qzone::parse_history_timestamp(text);
            if parsed > 0 {
                return parsed;
            }
        }
    }
    0
}

pub(super) fn parsed_library_page(
    module: &str,
    mut value: Value,
) -> Result<ParsedLibraryPage, String> {
    let data = if value.get("data").is_some() {
        value.get_mut("data").expect("checked data field")
    } else {
        &mut value
    };
    let (array_names, total_names): (&[&str], &[&str]) = match module {
        "albums" => (&["albumList", "albumsInUser"], &["total", "albumTotal"]),
        "photos" => (&["photoList", "photos"], &["totalInAlbum", "total"]),
        "videos" => (&["Videos", "videos"], &["total", "videoTotal"]),
        "guestbook" => (&["commentList"], &["total", "commentTotal"]),
        "favorites" => (&["fav_list"], &["total_num", "total"]),
        _ => (&[], &[]),
    };
    let total = total_names
        .iter()
        .find_map(|name| data.get(*name))
        .map(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str()?.parse().ok())
                .and_then(|count| usize::try_from(count).ok())
                .ok_or_else(|| format!("{module} 响应总数字段无效"))
        })
        .transpose()?;
    let name = array_names
        .iter()
        .find(|name| data.get(**name).is_some_and(Value::is_array))
        .ok_or_else(|| format!("{module} 响应缺少有效列表字段，已保留进度"))?;
    let items = std::mem::take(
        data.get_mut(*name)
            .and_then(Value::as_array_mut)
            .expect("validated list"),
    );
    if items.iter().any(|item| !item.is_object()) {
        return Err(format!("{module} 列表含无效记录，已保留断点"));
    }
    Ok(ParsedLibraryPage { items, total })
}

pub(super) fn library_page_complete(
    offset: usize,
    count: usize,
    total: Option<usize>,
) -> Result<bool, String> {
    if count == 0 {
        if total.is_some_and(|total| offset < total) {
            return Err(format!(
                "资料分页在 {offset} 提前返回空页，服务端仍声明存在记录，已保留断点"
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

fn library_item_key(module: &str, raw: &Value) -> String {
    let key_names: &[&str] = match module {
        "albums" => &["id", "albumid", "albumId", "topicId"],
        "photos" => &["lloc", "sloc", "picKey", "id", "photoId"],
        "videos" => &["vid", "shuoshuoid", "id", "videoId"],
        "guestbook" => &["id", "commentid", "commentId", "sign"],
        "favorites" => &["id", "fav_id", "favId", "key"],
        _ => &[],
    };
    value_text(raw, key_names).unwrap_or_else(|| {
        let identity = if module == "guestbook" {
            serde_json::json!([
                raw.get("uin"),
                raw.get("pubtime"),
                library_content::guestbook_content(raw)
            ])
        } else {
            raw.clone()
        };
        format!("fallback:{:016x}", stable_feed_hash(&identity))
    })
}

struct LibraryItemFields {
    item_key: String,
    created_at: i64,
    title: String,
    summary: String,
    author_uin: Option<String>,
    author_name: Option<String>,
    cover_url: Option<String>,
    media_urls: Vec<String>,
}

fn library_item_fields(module: &str, raw: &Value) -> LibraryItemFields {
    if matches!(module, "guestbook" | "favorites") {
        let images = library_content::images(module, raw);
        let info = library_content::favorite_info(raw);
        let author_uin = value_text(raw, &["uin"]).or_else(|| value_text(info, &["owner_uin"]));
        let author_name = value_text(raw, &["nickname", "nick"])
            .or_else(|| value_text(info, &["owner_name", "owner_nam"]));
        let title = if module == "guestbook" {
            author_name.clone().unwrap_or_else(|| "空间留言".into())
        } else {
            library_content::text_content(
                &value_text(raw, &["title"]).unwrap_or_else(|| "收藏内容".into()),
            )
        };
        let summary = if module == "guestbook" {
            library_content::guestbook_content(raw)
        } else {
            library_content::text_content(
                &value_text(raw, &["abstract", "desp"]).unwrap_or_default(),
            )
        };
        return LibraryItemFields {
            item_key: library_item_key(module, raw),
            created_at: library_created_at(raw),
            title,
            summary,
            author_uin,
            author_name,
            cover_url: images.first().map(|image| image.thumbnail.clone()),
            media_urls: images.into_iter().map(|image| image.url).collect(),
        };
    }
    let item_key = library_item_key(module, raw);
    let created_at = library_created_at(raw);
    let title_names: &[&str] = match module {
        "albums" => &["name", "title"],
        "photos" => &["name", "title", "desc"],
        "videos" => &["title", "name"],
        _ => &[],
    };
    let summary_names: &[&str] = match module {
        "albums" | "photos" => &["desc", "description", "name"],
        "videos" => &["desc", "description", "title", "name"],
        _ => &[],
    };
    let title = value_text(raw, title_names).unwrap_or_else(|| {
        match module {
            "albums" => "未命名相册",
            "photos" => "未命名照片",
            "videos" => "未命名视频",
            _ => "归档内容",
        }
        .into()
    });
    let summary =
        library_content::text_content(&value_text(raw, summary_names).unwrap_or_default());
    let author_uin = value_text(raw, &["uin", "author_uin", "ownerUin", "custom_uin"]);
    let author_name = value_text(raw, &["nickname", "nick", "author_name", "custom_name"]);
    let (cover_url, media_urls) = gallery::resource_media(module, raw);
    LibraryItemFields {
        item_key,
        created_at,
        title,
        summary,
        author_uin,
        author_name,
        cover_url,
        media_urls,
    }
}

pub(super) fn save_library_page(
    transaction: &rusqlite::Transaction<'_>,
    owner_uin: &str,
    module: &str,
    scope_key: &str,
    parent_key: &str,
    raw_items: &[Value],
) -> Result<u64, String> {
    let mut saved = 0_u64;
    let archived_at = now();
    let mut statement = transaction.prepare_cached(
            "INSERT INTO archive_library_items
             (owner_uin,module,scope_key,item_key,parent_key,created_at,title,summary,author_uin,author_name,cover_url,media_json,raw_json,archived_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(owner_uin,module,scope_key,item_key) DO UPDATE SET
               parent_key=excluded.parent_key,
               created_at=CASE WHEN excluded.created_at>0 THEN excluded.created_at ELSE archive_library_items.created_at END,
               title=CASE WHEN excluded.title<>'' THEN excluded.title ELSE archive_library_items.title END,
               summary=CASE WHEN excluded.summary<>'' THEN excluded.summary ELSE archive_library_items.summary END,
               author_uin=COALESCE(excluded.author_uin,archive_library_items.author_uin),
               author_name=COALESCE(excluded.author_name,archive_library_items.author_name),
               cover_url=COALESCE(excluded.cover_url,archive_library_items.cover_url),
               media_json=CASE WHEN excluded.media_json<>'[]' THEN excluded.media_json ELSE archive_library_items.media_json END,
               raw_json=excluded.raw_json,archived_at=excluded.archived_at",
        ).map_err(|error| format!("准备资料归档写入失败：{error}"))?;
    for raw in raw_items {
        if module == "guestbook" && library_content::guestbook_content(raw).trim().is_empty() {
            continue;
        }
        let LibraryItemFields {
            item_key,
            created_at,
            title,
            summary,
            author_uin,
            author_name,
            cover_url,
            media_urls,
        } = library_item_fields(module, raw);
        statement
            .execute(params![
                owner_uin,
                module,
                scope_key,
                item_key,
                parent_key,
                created_at,
                title,
                summary,
                author_uin,
                author_name,
                cover_url,
                serde_json::to_string(&media_urls).map_err(|e| e.to_string())?,
                raw.to_string(),
                archived_at
            ])
            .map_err(|error| format!("保存资料归档失败：{error}"))?;
        saved += 1;
    }
    Ok(saved)
}

pub(super) fn save_library_state(
    connection: &Connection,
    owner_uin: &str,
    module: &str,
    scope_key: &str,
    remote_total: usize,
    fetched: usize,
    complete: bool,
) -> Result<(), String> {
    connection.execute(
        "INSERT INTO archive_library_state(owner_uin,module,scope_key,remote_total,fetched,complete,last_error,synced_at)
         VALUES (?1,?2,?3,?4,?5,?6,NULL,?7)
         ON CONFLICT(owner_uin,module,scope_key) DO UPDATE SET
           remote_total=excluded.remote_total,fetched=excluded.fetched,complete=excluded.complete,
           last_error=excluded.last_error,synced_at=excluded.synced_at",
        params![owner_uin,module,scope_key,remote_total as i64,fetched as i64,complete,now()],
    ).map_err(|error| format!("保存资料同步状态失败：{error}"))?;
    Ok(())
}

fn save_library_error(
    connection: &Connection,
    owner: &str,
    module: &str,
    scope: &str,
    error: Option<&str>,
) -> Result<(), String> {
    connection.execute(
        "INSERT INTO archive_library_state(owner_uin,module,scope_key,remote_total,fetched,complete,last_error,synced_at)
         VALUES (?1,?2,?3,0,0,0,?4,0)
         ON CONFLICT(owner_uin,module,scope_key) DO UPDATE SET last_error=excluded.last_error",
        params![owner,module,scope,error],
    ).map_err(|error| format!("记录资料同步状态失败：{error}"))?;
    Ok(())
}

#[tauri::command]
pub async fn sync_qzone_library(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    page_token: tauri::State<'_, qzone::QzonePageTokenState>,
    archive: tauri::State<'_, ArchiveState>,
    module: String,
    parent_key: Option<String>,
    task_id: Option<String>,
) -> Result<LibrarySyncResult, String> {
    validate_library_module(&module)?;
    let scope_key = parent_key.unwrap_or_default();
    if module == "photos" && scope_key.trim().is_empty() {
        return Err("请选择需要同步的相册".into());
    }
    let _permit = archive
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
        2_000,
    );
    *archive.progress.lock().map_err(|_| "归档状态锁已损坏")? = ArchiveProgress {
        owner_uin: Some(owner),
        task_id,
        status: "running",
        active: true,
        stage: "library",
        message: "正在同步资料…".into(),
        ..Default::default()
    };
    let mut result = run_library_sync(
        &app,
        &login,
        &page_token,
        &archive,
        &session,
        module.clone(),
        scope_key.clone(),
    )
    .await;
    if let Err(error) = &result {
        let error = match error.as_str() {
            "ARCHIVE_PAUSED" | "ARCHIVE_ABANDONED" => None,
            _ => Some(concise_archive_error(error)),
        };
        let db_app = app.clone();
        let db_owner = session.owner.clone();
        if let Err(state_error) = blocking(move || {
            let db = open_database(&db_app)?;
            save_library_error(&db, &db_owner, &module, &scope_key, error.as_deref())
        })
        .await
        {
            if let Err(original) = &result {
                result = Err(format!("{original}；{state_error}"));
            }
        }
    }
    set_progress(&archive, |p| {
        p.active = false;
        match &result {
            Ok(result) => {
                p.status = "incomplete";
                p.message = result.message.clone();
            }
            Err(error) => {
                p.status = match error.as_str() {
                    "ARCHIVE_PAUSED" => "paused",
                    "ARCHIVE_ABANDONED" => "abandoned",
                    e if e.starts_with("ARCHIVE_RATE_LIMIT:") => "limited",
                    _ => "error",
                };
                p.message = format!(
                    "资料同步停止，已保存记录和断点：{}",
                    concise_archive_error(error)
                );
            }
        }
    });
    result
}

async fn run_library_sync(
    app: &tauri::AppHandle,
    login: &QLoginState,
    page_token: &qzone::QzonePageTokenState,
    archive: &ArchiveState,
    session: &Session,
    module: String,
    scope_key: String,
) -> Result<LibrarySyncResult, String> {
    let owner_uin = session.owner.clone();
    let db_app = app.clone();
    let db_owner = owner_uin.clone();
    let db_module = module.clone();
    let db_scope = scope_key.clone();
    let (mut offset, mut remote_total) = blocking(move || {
        let db = open_database(&db_app)?;
        db.query_row("SELECT CASE WHEN complete=0 THEN fetched ELSE 0 END, CASE WHEN complete=0 THEN remote_total ELSE 0 END FROM archive_library_state
            WHERE owner_uin=?1 AND module=?2 AND scope_key=?3", params![db_owner,db_module,db_scope],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)))
            .optional().map(|row| row.unwrap_or_default()).map_err(|e| e.to_string())
    }).await?;
    let mut saved = 0_u64;
    let mut previous_keys = Vec::new();
    loop {
        session.check()?;
        let response = session
            .network(qzone::fetch_library_page(
                app,
                login,
                page_token,
                &module,
                offset,
                Some(&scope_key),
                Some(session),
            ))
            .await?;
        let page = parsed_library_page(&module, response)?;
        if let Some(total) = page.total {
            remote_total = remote_total.max(total);
        }
        let keys = page
            .items
            .iter()
            .map(|item| library_item_key(&module, item))
            .collect::<Vec<_>>();
        if !keys.is_empty() && keys == previous_keys {
            return Err("接口重复返回同一页，断点未推进，未标记为完整".into());
        }
        previous_keys = keys;
        let page_len = page.items.len();
        offset = offset
            .checked_add(page_len)
            .ok_or("资料偏移超出整数表示范围")?;
        let complete =
            library_page_complete(offset, page_len, (remote_total > 0).then_some(remote_total))?;
        let db_app = app.clone();
        let db_owner = owner_uin.clone();
        let db_module = module.clone();
        let db_scope = scope_key.clone();
        saved += blocking(move || {
            let mut db = open_database(&db_app)?;
            let tx = db.transaction().map_err(|e| e.to_string())?;
            let saved = save_library_page(
                &tx,
                &db_owner,
                &db_module,
                &db_scope,
                &db_scope,
                &page.items,
            )?;
            save_library_state(
                &tx,
                &db_owner,
                &db_module,
                &db_scope,
                remote_total,
                offset,
                complete,
            )?;
            tx.commit()
                .map_err(|e| format!("提交资料与断点失败：{e}"))?;
            Ok(saved)
        })
        .await?;
        set_progress(archive, |p| {
            p.pages += 1;
            p.fetched += page_len as u64;
            p.message = format!("资料已读取 {offset} 条，继续同步…");
        });
        if complete {
            break;
        }
    }
    let fetched = offset;
    let message = format!("同步完成：接口读取 {fetched} 条，本地更新 {saved} 条");
    Ok(LibrarySyncResult {
        module,
        fetched: fetched as u64,
        saved,
        remote_total: remote_total as u64,
        complete: true,
        message,
    })
}

// One source projection drives counting, filtering and pagination. History rows
// remain separate because content equality does not establish record identity.
const LIBRARY_SOURCE: &str = "WITH source AS (
    SELECT id,module,item_key,parent_key,created_at,title,summary,author_uin,author_name,cover_url,media_json,
           CASE WHEN ?2 IN ('guestbook','favorites') THEN raw_json ELSE '{}' END AS raw_json,'current' AS source
    FROM archive_library_items WHERE owner_uin=?1 AND module=?2 AND scope_key=?3
    UNION ALL
    SELECT -id,'guestbook','history:'||cell_id,'',published_at,
           COALESCE(NULLIF(author_name,''),'空间留言'),COALESCE(content,''),author_uin,author_name,NULL,'[]','{}','history'
    FROM archive_dynamics WHERE owner_uin=?1 AND ?2='guestbook' AND ?3='' AND category='guestbook'
), filtered AS (
    SELECT * FROM source WHERE
        (?2 <> 'guestbook' OR length(trim(COALESCE(summary,''))) > 0)
        AND (?4 IS NULL OR CAST(strftime('%Y',created_at,'unixepoch','localtime') AS INTEGER)=?4)
        AND (?5 IS NULL OR instr(lower(title),lower(?5))>0 OR instr(lower(summary),lower(?5))>0
            OR instr(lower(COALESCE(author_name,'')),lower(?5))>0 OR instr(COALESCE(author_uin,''),?5)>0)
        AND (?6 IS NULL OR source=?6)
) ";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryQuery {
    module: String,
    parent_key: Option<String>,
    query: Option<String>,
    year: Option<i32>,
    source: Option<String>,
    limit: i64,
    offset: i64,
}

pub(super) fn read_library_page(
    connection: &Connection,
    owner_uin: &str,
    request: LibraryQuery,
) -> Result<LibraryPage, String> {
    let LibraryQuery {
        module,
        parent_key,
        query,
        year,
        source,
        limit,
        offset,
    } = request;
    let scope_key = parent_key.unwrap_or_default();
    if source
        .as_deref()
        .is_some_and(|source| !matches!(source, "current" | "history"))
    {
        return Err("无效的留言来源筛选".into());
    }
    let search = query
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let total = connection
        .query_row(
            &format!("{LIBRARY_SOURCE} SELECT COUNT(*) FROM filtered"),
            params![owner_uin, module, scope_key, year, search, source],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("统计资料归档失败：{error}"))?;
    let mut statement = connection.prepare(&format!("{LIBRARY_SOURCE}
        SELECT id,module,item_key,parent_key,created_at,title,summary,author_uin,author_name,cover_url,media_json,raw_json,source
        FROM filtered ORDER BY created_at DESC,id DESC LIMIT ?7 OFFSET ?8"))
        .map_err(|error| format!("准备资料归档查询失败：{error}"))?;
    let rows = statement
        .query_map(
            params![
                owner_uin,
                module,
                scope_key,
                year,
                search,
                source,
                limit.clamp(1, 200),
                offset.max(0)
            ],
            |row| {
                let module: String = row.get(1)?;
                let media_json: String = row.get(10)?;
                let raw_json: String = row.get(11)?;
                let origin: String = row.get(12)?;
                let decode = |index, text: &str| {
                    serde_json::from_str::<Value>(text).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            index,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                };
                let raw = decode(11, &raw_json)?;
                let media_urls = serde_json::from_str(&media_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(LibraryItem {
                    id: row.get(0)?,
                    item_key: row.get(2)?,
                    parent_key: row.get(3)?,
                    created_at: row.get(4)?,
                    title: row.get(5)?,
                    summary: row.get(6)?,
                    author_uin: row.get(7)?,
                    author_name: row.get(8)?,
                    cover_url: row.get(9)?,
                    media_urls,
                    details: library_content::details(&module, &raw, origin == "history"),
                    module,
                })
            },
        )
        .map_err(|error| format!("查询资料归档失败：{error}"))?;
    let items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取资料归档失败：{error}"))?;
    let state = connection.query_row(
        "SELECT remote_total,complete,synced_at,last_error FROM archive_library_state WHERE owner_uin=?1 AND module=?2 AND scope_key=?3",
        params![owner_uin,module,scope_key],
        |row| Ok((row.get::<_,i64>(0)?,row.get::<_,bool>(1)?,row.get::<_,i64>(2)?,row.get::<_,Option<String>>(3)?)),
    ).optional().map_err(|error| format!("读取同步状态失败：{error}"))?.unwrap_or((0,false,0,None));
    Ok(LibraryPage {
        items,
        total: total.max(0) as u64,
        remote_total: state.0.max(0) as u64,
        complete: state.1,
        synced_at: state.2,
        last_error: state.3,
    })
}

#[tauri::command]
pub async fn list_qzone_library(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    request: LibraryQuery,
) -> Result<LibraryPage, String> {
    validate_library_module(&request.module)?;
    let owner_uin = login.qzone_auth().await?.uin;
    blocking(move || {
        let connection = open_database(&app)?;
        read_library_page(&connection, &owner_uin, request)
    })
    .await
}

#[tauri::command]
pub async fn list_qzone_library_years(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    module: String,
    parent_key: Option<String>,
) -> Result<Vec<i32>, String> {
    validate_library_module(&module)?;
    let owner_uin = login.qzone_auth().await?.uin;
    let scope_key = parent_key.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let connection = open_database(&app)?;
        let mut statement = connection.prepare(
            "WITH timestamps AS (
               SELECT created_at FROM archive_library_items
               WHERE owner_uin=?1 AND module=?2 AND scope_key=?3 AND created_at>0
               UNION
               SELECT published_at FROM archive_dynamics
               WHERE owner_uin=?1 AND ?2='guestbook' AND ?3='' AND category='guestbook' AND published_at>0
             )
             SELECT DISTINCT CAST(strftime('%Y',created_at,'unixepoch','localtime') AS INTEGER)
             FROM timestamps ORDER BY 1 DESC"
        ).map_err(|error| format!("读取资料归档年份失败：{error}"))?;
        let rows = statement.query_map(params![owner_uin,module,scope_key], |row| row.get(0))
            .map_err(|error| format!("查询资料归档年份失败：{error}"))?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error| format!("读取资料归档年份失败：{error}"))
    }).await.map_err(|error| format!("资料归档年份任务异常退出：{error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_filter_and_paging_keep_equal_content_records_and_account_boundaries() {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::initialize_database(&mut db).unwrap();
        let tx = db.transaction().unwrap();
        save_library_page(&tx, "100", "guestbook", "", "", &[
            json!({"id":"a","uin":"200","nickname":"朋友","pubtime":1700000000,"htmlContent":"<p>相同的留言</p>","replyList":[{"uin":"100","pubtime":1700000100,"htmlContent":"记得"}]}),
            json!({"id":"b","uin":"200","nickname":"朋友","pubtime":1700000000,"htmlContent":"相同的留言"}),
        ]).unwrap();
        save_library_page(
            &tx,
            "999",
            "guestbook",
            "",
            "",
            &[json!({"id":"other","htmlContent":"其他账号"})],
        )
        .unwrap();
        tx.execute("INSERT INTO archive_dynamics(owner_uin,cell_id,category,published_at,content,raw_original_json,archived_at) VALUES ('100','history-a','guestbook',1700000000,'相同的留言','{}',1)", []).unwrap();
        tx.commit().unwrap();
        let page = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":2, "offset":0})).unwrap(),
        )
        .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        let tail = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":2, "offset":2})).unwrap(),
        )
        .unwrap();
        assert_eq!(tail.items.len(), 1);
        assert_eq!(tail.items[0].details.source, "history");
        let history = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":24, "offset":0, "query": "相同", "source": "history"})).unwrap(),
        )
        .unwrap();
        assert_eq!(history.total, 1);
        let current = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":24, "offset":0, "query": "朋友", "source": "current"})).unwrap(),
        )
        .unwrap();
        assert_eq!(current.total, 2);
        assert_eq!(
            current
                .items
                .iter()
                .map(|item| item.details.replies.len())
                .sum::<usize>(),
            1
        );
        assert_eq!(current.items[0].summary, "相同的留言");
        save_library_state(&db, "100", "guestbook", "", 3, 2, false).unwrap();
        save_library_error(&db, "100", "guestbook", "", Some("网络读取失败")).unwrap();
        let failed = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":24, "offset":0})).unwrap(),
        )
        .unwrap();
        assert_eq!(failed.last_error.as_deref(), Some("网络读取失败"));
        assert_eq!(failed.total, 3);
        assert!(!failed.complete);
        assert_eq!(db.query_row("SELECT fetched FROM archive_library_state WHERE owner_uin='100' AND module='guestbook'", [], |row| row.get::<_,i64>(0)).unwrap(), 2);
        assert!(parsed_library_page("guestbook", json!({"data":{"commentList":[null]}})).is_err());
    }

    #[test]
    fn a_new_reply_does_not_duplicate_guestbook_records_without_server_ids() {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::initialize_database(&mut db).unwrap();
        let mut item = json!({"uin":"200","pubtime":1700000000,"htmlContent":"同一条留言"});
        let tx = db.transaction().unwrap();
        save_library_page(&tx, "100", "guestbook", "", "", &[item.clone()]).unwrap();
        item["replyList"] = json!([{"uin":"100","htmlContent":"新的回复"}]);
        save_library_page(&tx, "100", "guestbook", "", "", &[item]).unwrap();
        tx.commit().unwrap();
        let page = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":24, "offset":0})).unwrap(),
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].details.replies.len(), 1);
    }

    #[test]
    fn guestbook_omits_records_without_readable_body() {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::initialize_database(&mut db).unwrap();
        let tx = db.transaction().unwrap();
        save_library_page(
            &tx,
            "100",
            "guestbook",
            "",
            "",
            &[
                json!({"id":"empty-current","htmlContent":"<p> \u{a0} </p>"}),
                json!({"id":"visible-current","htmlContent":"<p>可见留言</p>"}),
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO archive_dynamics(owner_uin,cell_id,category,published_at,content,raw_original_json,archived_at)
             VALUES ('100','empty-history','guestbook',1700000000,'   ','{}',1)",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
        let page = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"guestbook", "limit":24, "offset":0})).unwrap(),
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].summary, "可见留言");
    }

    #[test]
    fn favorites_keep_all_images_and_do_not_turn_webpage_links_into_covers() {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::initialize_database(&mut db).unwrap();
        let raw = json!({"id":"favorite","type":1,"create_time":1700000000,"title":"<b>收藏网页</b>","abstract":"<p>第一行</p><p>第二行</p>",
            "url_info":{"url":"https://example.com/article"}, "origin_img_list":(0..160).map(|index| format!("https://qpic.cn/{index}.jpg")).collect::<Vec<_>>()});
        let tx = db.transaction().unwrap();
        save_library_page(&tx, "100", "favorites", "", "", &[raw]).unwrap();
        tx.commit().unwrap();
        let page = read_library_page(
            &db,
            "100",
            serde_json::from_value(json!({"module":"favorites", "limit":24, "offset":0})).unwrap(),
        )
        .unwrap();
        let item = &page.items[0];
        assert_eq!(item.title, "收藏网页");
        assert_eq!(item.summary, "第一行\n第二行");
        assert_eq!(item.details.images.len(), 160);
        assert_eq!(item.media_urls.len(), 160);
        assert_eq!(
            item.details.source_url.as_deref(),
            Some("https://example.com/article")
        );
        assert!(item
            .cover_url
            .as_ref()
            .unwrap()
            .starts_with("https://qpic.cn/"));
        assert!(parsed_library_page("favorites", json!({"data":{}})).is_err());
    }
}
