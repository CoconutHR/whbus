//! 应用日志：环形缓冲 + 落盘轮转。
//!
//! # 为什么要自己做
//!
//! 项目里一直有 `log` crate，但**没有装任何 logger 后端** ——
//! 于是所有 `log::warn!` 都直接丢掉。用户遇到"暂时给不出方案"时
//! 拿不到任何线索，只能靠猜。
//!
//! `tauri-plugin-log` 能解决落盘，但不方便让 UI 展示"最近 N 条"并
//! 按需导出；而排查这类问题恰恰要的是"刚才那次失败到底卡在哪一步"。
//! 所以这里实现一个最小的双通道 logger：
//!
//! - **内存环形缓冲**：始终保留最近 `CAPACITY` 条，UI 直接读取，零 IO
//! - **文件追加**：超过 `MAX_FILE_BYTES` 就轮转一次（只留一个 `.1`）
//!
//! # 隐私
//!
//! 日志里会出现坐标、站名、线路号，但**不会**出现高德 Key
//! （`keyfile` 与 `amap` 都不把 Key 写进日志）与设备标识。导出前
//! UI 会明确告知包含哪些内容。

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// 内存中保留的日志条数
const CAPACITY: usize = 600;

/// 供 get_logs 限制上限使用（UI 不该一次要十万条）
pub const CAPACITY_PUBLIC: usize = CAPACITY;
/// 单个日志文件的上限（字节）
const MAX_FILE_BYTES: u64 = 512 * 1024;

/// 一条日志
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// 毫秒时间戳
    pub ts: i64,
    /// 级别：ERROR / WARN / INFO / DEBUG / TRACE
    pub level: String,
    /// 模块，如 `whbus::commands`
    pub target: String,
    pub message: String,
}

struct Logger {
    ring: Mutex<VecDeque<LogEntry>>,
    /// 日志文件路径；None 表示尚未初始化（初始化前只进内存）
    file: OnceLock<PathBuf>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

fn logger() -> &'static Logger {
    LOGGER.get_or_init(|| Logger {
        ring: Mutex::new(VecDeque::with_capacity(CAPACITY)),
        file: OnceLock::new(),
    })
}

/// 当前时间戳（毫秒）
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 把毫秒时间戳格式化成 `YYYY-MM-DD HH:MM:SS.mmm`（本地时区）
fn stamp(ms: i64) -> String {
    // 不引 chrono：只需要一个可读时间戳，用整秒差与手写日历换算足够
    crate::transit::format_local_ms(ms)
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // DEBUG 在 release 下也保留：出问题时用户装的就是 release 包，
        // 恰恰只有 release 的日志有用
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let entry = LogEntry {
            ts: now_ms(),
            level: record.level().to_string(),
            target: record.target().to_string(),
            message: format!("{}", record.args()),
        };

        // 环形缓冲
        if let Ok(mut ring) = self.ring.lock() {
            if ring.len() >= CAPACITY {
                ring.pop_front();
            }
            ring.push_back(entry.clone());
        }

        // 文件追加（失败就算了，日志不该影响主流程）
        if let Some(path) = self.file.get() {
            let line = format!(
                "{} [{}] {}: {}\n",
                stamp(entry.ts),
                entry.level,
                entry.target,
                entry.message
            );
            let _ = append_with_rotation(path, &line);
        }
    }

    fn flush(&self) {}
}

/// 追加一行；超过上限就把现有内容转存到 `.1` 再重开
fn append_with_rotation(path: &PathBuf, line: &str) -> std::io::Result<()> {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_FILE_BYTES {
            let mut rotated = path.clone();
            rotated.set_extension("log.1");
            // 覆盖旧的历史文件：只保留一代，避免无限增长
            let _ = std::fs::rename(path, rotated);
        }
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())
}

/// 安装全局 logger。
///
/// 必须在任何 `log::*` 调用之前执行。
///
/// 用 `set_logger` + 静态引用，而不是 `set_boxed_logger` ——
/// 后者需要 `log` 的 `std` feature，没必要为它开。
pub fn install() {
    let _ = log::set_logger(logger());
    log::set_max_level(LevelFilter::Debug);
}

/// 让日志同时写入 `dir/whbus.log`
pub fn attach_file(dir: PathBuf) {
    let path = dir.join("whbus.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 只设置一次；重复调用无害
    let _ = logger().file.set(path);
}

/// 读取最近 `limit` 条（最新的在最后）
pub fn recent(limit: usize) -> Vec<LogEntry> {
    logger()
        .ring
        .lock()
        .map(|ring| {
            let n = ring.len();
            ring.iter()
                .skip(n.saturating_sub(limit))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// 日志文件路径（若已附加）
pub fn file_path() -> Option<PathBuf> {
    logger().file.get().cloned()
}

/// 清空内存缓冲（文件保留，便于对照）
pub fn clear() {
    if let Ok(mut ring) = logger().ring.lock() {
        ring.clear();
    }
}

/// 把最近日志导出成一个文本文件，返回路径。
///
/// 放在应用数据目录而不是临时目录：安卓上临时目录可能被系统清理，
/// 而用户往往过一会儿才去分享。
pub fn export() -> std::io::Result<PathBuf> {
    let dir = file_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);
    let out = dir.join("whbus-log-export.txt");

    let entries = recent(CAPACITY);
    let mut s = String::new();
    s.push_str("# 武汉公交 App 日志导出\n");
    s.push_str(&format!("# 导出时间：{}\n", stamp(now_ms())));
    s.push_str(&format!("# 条数：{}\n", entries.len()));
    s.push_str(&format!("# 版本：{}\n", env!("CARGO_PKG_VERSION")));
    s.push_str("# 注意：包含坐标与站名，分享前请自行确认\n\n");
    for e in &entries {
        s.push_str(&format!(
            "{} [{}] {}: {}\n",
            stamp(e.ts),
            e.level,
            e.target,
            e.message
        ));
    }
    std::fs::write(&out, s)?;
    Ok(out)
}
