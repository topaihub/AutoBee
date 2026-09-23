//! 建表、迁移与初始数据。
//!
//! # 迁移靠 `PRAGMA user_version`
//!
//! SQLite 自带一个存在文件头里的整数版本号，够用而且不占一张表。
//! 加字段时：把 [`VERSION`] 加一，在 [`migrate`] 里加一个 `if version < N` 分支。
//! **旧版本的 db 必须能被新版本读** —— 用户装了新版程序不会先删 db。

use rusqlite::Connection;

use crate::error::AppError;

/// 当前的库结构版本。加字段时加一。
const VERSION: i64 = 4;

/// 建表 SQL。
///
/// `IF NOT EXISTS` 让它可以在每次开库时无条件跑一遍 ——
/// 判断「建过没有」再决定要不要建，那个判断本身就是要维护的状态。
const CREATE: &str = "
CREATE TABLE IF NOT EXISTS map_providers (
  slug        TEXT    PRIMARY KEY,
  name        TEXT    NOT NULL,
  is_preset   INTEGER NOT NULL DEFAULT 0,
  enabled     INTEGER NOT NULL DEFAULT 1,
  priority    INTEGER,                     -- NULL = 用注册表里的默认值
  api_key_enc BLOB,                        -- DPAPI 密文，NULL = 还没填
  sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS prefs (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

/// 预置的六家。
///
/// 密钥留空 —— 我们不知道用户的 Key，也不该凭空造一个。
/// `is_preset = 1` 只影响两件事：不允许删，以及排在用户自建的前面。
const SEED: &str = "
INSERT OR IGNORE INTO map_providers (slug, name, is_preset, sort_order) VALUES
  ('tencent',  '腾讯位置服务', 1, 0),
  ('amap',     '高德地图',     1, 1),
  ('tianditu', '天地图',       1, 2),
  ('baidu',    '百度地图',     1, 3),
  ('here',     'HERE',         1, 4),
  ('google',   'Google Maps',  1, 5);
";

/// 建表、跑迁移、必要时插入初始数据。
///
/// # Errors
///
/// SQL 失败时返回错误 —— 那说明 db 损坏或磁盘满，不能当「没有档案」继续
/// （那会让用户以为档案丢了，然后重新填一遍存不进去）。
pub(crate) fn prepare(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(CREATE)
        .map_err(|error| sql_error("建表", &error))?;

    let version = user_version(conn)?;
    if version == 0 {
        // 只在全新的库里插预置数据。每次开库都插的话，用户删掉的那条
        // 重启程序又会长回来 —— 那种「删不掉」最让人以为程序有 bug。
        conn.execute_batch(SEED)
            .map_err(|error| sql_error("写入预置服务商", &error))?;
    }
    migrate(conn, version)?;
    set_user_version(conn, VERSION)?;
    Ok(())
}

/// 从 `from` 版本迁到 [`VERSION`]。
///
/// `from == 0` 是全新库，不进迁移分支；建表 SQL 已经是最新结构。
fn migrate(conn: &Connection, from: i64) -> Result<(), AppError> {
    if from < 3 {
        conn.execute(
            "UPDATE prefs SET value = 'arcgis-street' WHERE key = 'tile_source' AND value IN ('carto-light', 'carto-dark', 'osm')",
            [],
        )
        .map_err(|error| sql_error("迁移底图设置", &error))?;
    }
    if from < 4 {
        conn.execute(
            "UPDATE prefs SET value = 'GCJ02' WHERE key = 'out_crs' AND value = 'WGS84'",
            [],
        )
        .map_err(|error| sql_error("迁移坐标系设置", &error))?;
    }
    if from > VERSION {
        tracing::warn!(
            db_version = from,
            code_version = VERSION,
            "档案库来自更新的版本，可能有读不到的字段"
        );
    }
    Ok(())
}

/// 读 `PRAGMA user_version`。
fn user_version(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| sql_error("读库版本", &error))
}

/// 写 `PRAGMA user_version`。
///
/// 只能拼字符串：`PRAGMA` 不接受参数绑定。`version` 是本模块的常量，
/// 不是外部输入，所以这里没有注入面。
fn set_user_version(conn: &Connection, version: i64) -> Result<(), AppError> {
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))
        .map_err(|error| sql_error("写库版本", &error))
}

/// 把 SQL 错误折成带处置建议的错误。
///
/// 附上「删掉重建」的提示：db 是缓存性质的（档案能重填），
/// 而用户面对一个打不开的设置页时最需要知道的就是这条。
fn sql_error(doing: &str, error: &rusqlite::Error) -> AppError {
    tracing::error!(%error, doing, "档案库操作失败");
    AppError::new(
        "DB_ERROR",
        format!("服务商档案库{doing}失败：{error}。可以删掉 settings.db 后重启（档案要重填）。"),
        500,
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "测试里断言失败即测试失败"
)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        conn
    }

    #[test]
    fn 建表后有六家预置服务商() {
        let conn = memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM map_providers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 6);
    }

    #[test]
    fn 预置服务商不带密钥() {
        let conn = memory();
        let filled: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM map_providers WHERE api_key_enc IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(filled, 0, "我们不知道用户的 Key");
    }

    #[test]
    fn 重复_prepare_不会插出第二份() {
        let conn = Connection::open_in_memory().unwrap();
        for _ in 0..3 {
            prepare(&conn).unwrap();
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM map_providers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 6);
    }

    #[test]
    fn 删掉预置的服务商后重开不会长回来() {
        let conn = memory();
        conn.execute("DELETE FROM map_providers WHERE slug = 'amap'", [])
            .unwrap();
        prepare(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM map_providers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn 库版本被写上() {
        // 不写的现象是每次开库都当成全新库，预置数据一直往里插。
        assert_eq!(user_version(&memory()).unwrap(), VERSION);
    }

    #[test]
    fn 来自更新版本的库不报错() {
        let conn = memory();
        set_user_version(&conn, VERSION + 3).unwrap();
        assert!(prepare(&conn).is_ok(), "只警告，不拒绝");
    }

    #[test]
    fn 偏好表可用() {
        let conn = memory();
        conn.execute(
            "INSERT INTO prefs (key, value) VALUES ('tile_source', 'osm')",
            [],
        )
        .unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM prefs WHERE key = 'tile_source'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "osm");
    }
}
