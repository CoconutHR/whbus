//! 高德 Key 的本地持久化。
//!
//! # 为什么不写进代码或版本库
//!
//! 1. 本仓库是**公开**的，硬编码的 Key 会被爬虫抓走、配额被刷爆
//! 2. 别人 clone 这个项目时应该用自己的配额
//! 3. 即使塞进 APK 里也谈不上安全——但至少不要主动公开
//!
//! 因此 Key 由用户在设置页粘贴，保存在应用私有目录：
//! - 安卓：`/data/data/<包名>/files/amap_key`（其他应用无法读取）
//! - 桌面：系统应用数据目录
//!
//! 文件只放一行 Key，不放任何其他内容。

use std::path::PathBuf;

use tauri::Manager;

use crate::{AppError, Result};

/// Key 文件名
const FILE: &str = "amap_key";

fn path(app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("无法定位应用数据目录：{e}")))?;
    Ok(dir.join(FILE))
}

/// 保存 Key（覆盖写）
pub fn save(app: &tauri::AppHandle, key: &str) -> Result<()> {
    let p = path(app)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("创建数据目录失败：{e}")))?;
    }
    std::fs::write(&p, key.trim())
        .map_err(|e| AppError::Internal(format!("保存高德 Key 失败：{e}")))?;

    // 安卓/Linux 上收紧权限：仅属主可读写
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// 读取 Key。文件不存在或为空时返回 `None`（不是错误——用户还没配置而已）。
pub fn load(app: &tauri::AppHandle) -> Option<String> {
    let p = path(app).ok()?;
    let s = std::fs::read_to_string(p).ok()?;
    let t = s.trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// 删除 Key
pub fn remove(app: &tauri::AppHandle) -> Result<()> {
    let p = path(app)?;
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(()),
        // 本来就不存在 → 视为成功
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::Internal(format!("删除高德 Key 失败：{e}"))),
    }
}
