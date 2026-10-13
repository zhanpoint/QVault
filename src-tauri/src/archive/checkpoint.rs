//! Offset stage snapshots are committed in the same transaction as their records.
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Default, Serialize, Deserialize)]
pub struct StageCheckpoint {
    pub offset: u64,
    pub pages: u32,
    pub records: u64,
    pub saved: u64,
    pub total: u64,
    pub owner_name: Option<String>,
    pub complete: bool,
}

pub fn load(app: &tauri::AppHandle, owner: &str, stage: &str) -> Result<StageCheckpoint, String> {
    let db = super::open_database(app)?;
    read(&db, owner, stage)
}

pub(super) fn read(db: &Connection, owner: &str, stage: &str) -> Result<StageCheckpoint, String> {
    let snapshot: Option<String> = db
        .query_row(
            "SELECT snapshot FROM archive_stage_checkpoints WHERE owner_uin=?1 AND stage=?2",
            params![owner, stage],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("读取阶段断点失败：{e}"))?;
    snapshot
        .map(|value| serde_json::from_str(&value).map_err(|e| format!("解析阶段断点失败：{e}")))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub fn write(
    tx: &Transaction<'_>,
    owner: &str,
    stage: &str,
    snapshot: &StageCheckpoint,
) -> Result<(), String> {
    let json = serde_json::to_string(snapshot).map_err(|e| format!("序列化阶段断点失败：{e}"))?;
    tx.execute(
        "INSERT INTO archive_stage_checkpoints(owner_uin,stage,snapshot) VALUES (?1,?2,?3)
        ON CONFLICT(owner_uin,stage) DO UPDATE SET snapshot=excluded.snapshot",
        params![owner, stage, json],
    )
    .map_err(|e| format!("保存阶段断点失败：{e}"))?;
    Ok(())
}

pub async fn save(
    app: &tauri::AppHandle,
    owner: &str,
    stage: &'static str,
    feeds: Vec<Value>,
    mut snapshot: StageCheckpoint,
) -> Result<StageCheckpoint, String> {
    let app = app.clone();
    let owner = owner.to_owned();
    super::blocking(move || {
        let mut db = super::open_database(&app)?;
        let tx = db
            .transaction()
            .map_err(|e| format!("开始阶段事务失败：{e}"))?;
        snapshot.saved += super::save_feed_rows(&tx, &owner, &feeds)?;
        write(&tx, &owner, stage, &snapshot)?;
        tx.commit().map_err(|e| format!("提交阶段事务失败：{e}"))?;
        Ok(snapshot)
    })
    .await
}

pub fn clear(db: &Connection, owner: &str) -> Result<(), String> {
    db.execute(
        "DELETE FROM archive_stage_checkpoints WHERE owner_uin=?1",
        [owner],
    )
    .map_err(|e| format!("清除阶段断点失败：{e}"))?;
    db.execute(
        "DELETE FROM archive_checkpoints WHERE owner_uin=?1",
        [owner],
    )
    .map_err(|e| format!("清除互动断点失败：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_rolls_back_with_failed_page_and_is_account_scoped() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE archive_stage_checkpoints(owner_uin TEXT,stage TEXT,snapshot TEXT,PRIMARY KEY(owner_uin,stage));
            CREATE TABLE records(id INTEGER PRIMARY KEY);").unwrap();
        {
            let tx = db.transaction().unwrap();
            tx.execute("INSERT INTO records VALUES (1)", []).unwrap();
            write(
                &tx,
                "100",
                "visible",
                &StageCheckpoint {
                    offset: 30,
                    ..Default::default()
                },
            )
            .unwrap();
            // Simulate a failed page commit: neither rows nor cursor can advance.
        }
        assert_eq!(read(&db, "100", "visible").unwrap().offset, 0);
        let tx = db.transaction().unwrap();
        tx.execute("INSERT INTO records VALUES (1)", []).unwrap();
        write(
            &tx,
            "100",
            "visible",
            &StageCheckpoint {
                offset: 30,
                ..Default::default()
            },
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(read(&db, "100", "visible").unwrap().offset, 30);
        assert_eq!(read(&db, "200", "visible").unwrap().offset, 0);
        db.execute(
            "INSERT INTO archive_stage_checkpoints VALUES ('200','visible','{\"offset\":999}')",
            [],
        )
        .unwrap();
        assert!(read(&db, "200", "visible").is_err());
        assert_eq!(read(&db, "100", "visible").unwrap().offset, 30);
    }
}
