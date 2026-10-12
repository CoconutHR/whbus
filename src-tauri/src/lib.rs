//! whbus —— 武汉公交实时通勤助手
//!
//! 架构要点：
//! - **坐标单一事实源**：App 内部全程使用 GCJ-02（火星坐标）。
//!   公交 API 返回 GCJ-02；定位插件返回 WGS-84，必须在边界处转换
//!   （武汉地区偏移达 ~587m，远超 nearBy 的 400m 搜索半径）。
//! - **加密在 Rust 侧**：公交 API 的 `data` 是 SM4-ECB 密文，密钥每日轮换。
//!   解密与密钥缓存全部留在后端，密钥永不进入 WebView。
//! - **网络在 Rust 侧**：所有外呼走 reqwest，规避 CORS 并统一超时/退避。

pub mod amap;
pub mod api;
pub mod commands;
pub mod gcj;
pub mod journey;
pub mod keyfile;
pub mod sm4crypto;
pub mod transit;
pub mod types;

pub use types::AppError;

/// 带超时的结果别名
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_haptics::init())
        .plugin(tauri_plugin_notification::init())
        // 共享状态：公交接口客户端、高德客户端、站点表与速度模型缓存
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_api_state,
            commands::search_lines,
            commands::search_stops,
            commands::get_line_detail,
            commands::get_line_vehicles,
            commands::get_nearby_stops,
            commands::get_stop_arrivals,
            commands::wgs84_to_gcj02_cmd,
            commands::gcj02_to_wgs84_cmd,
            commands::plan_journey,
            commands::get_amap_key_status,
            commands::set_amap_key,
            commands::search_place,
            commands::walking_route,
            commands::reverse_geocode,
        ])
        .setup(|app| {
            // geolocation 是 mobile-only 插件：桌面端不存在该 crate，
            // 因此注册必须放在 cfg(mobile) 内。
            #[cfg(mobile)]
            {
                use tauri::Manager;
                app.handle()
                    .plugin(tauri_plugin_geolocation::init())
                    .expect("failed to init geolocation plugin");
            }

            // 把磁盘上已保存的高德 Key 载入内存缓存。
            // 读不到是正常状态（用户还没配置），不算错误。
            if let Some(key) = keyfile::load(app.handle()) {
                use tauri::Manager;
                if let Some(state) = app.try_state::<commands::AppState>() {
                    if let Ok(mut g) = state.amap_key.lock() {
                        *g = Some(key);
                    }
                }
                log::info!("已载入本地保存的高德 Key");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
