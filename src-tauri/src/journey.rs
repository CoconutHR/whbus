//! 出行规划算法。
//!
//! # 现状
//!
//! 本模块的目标是"输入起点终点，输出若干可比方案"。当前实现的是
//! **一次换乘模型**：步行 → 公交 → 步行，或步行 → 公交 → 公交 → 步行。
//! 覆盖绝大多数通勤场景，且请求数可控。
//!
//! # 为什么不能做得更复杂（至少现在不能）
//!
//! 上游建议请求间隔 ≥ 10 秒。一次朴素的多换乘穷举会打出 40+ 个请求，
//! 既慢又容易触发限流。因此规划必须建立在**已缓存的站点表**之上：
//! 站点表（`/bus/line`）几乎不变，应当长期缓存；只有车辆位置
//! （`/bus/detailData`）需要高频刷新。
//!
//! # 待补
//!
//! - [ ] 候选站点枚举（依赖 `nearBy` 的 400m 半径，可能不足以覆盖家门到站距离）
//! - [ ] 站名 → `lineId` 的本地索引
//! - [ ] 多方案差异化标签（最快 / 少步行 / 更晚出发）
//! - [ ] "最晚几点出发"的逆向推导（由 `arrive_before` 反推）

use crate::types::{JourneyOption, JourneyQuery, LegKind};
use crate::Result;

/// 规划入口（尚未实现）。
///
/// 保留签名以便命令层与前端先行联调接口形态。
pub fn plan(_query: &JourneyQuery) -> Result<Vec<JourneyOption>> {
    Err(crate::AppError::Internal(
        "journey::plan 尚未实现".into(),
    ))
}

/// 方案排序维度。UI 上用标签区分，而不是只给一个排序列表——
/// 用户要的是"决策依据"，不是"一个答案"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankBias {
    /// 总耗时最短
    Fastest,
    /// 步行最少
    LeastWalking,
    /// 出发最晚（能多睡一会儿）
    LatestDeparture,
    /// 换乘最少
    FewestTransfers,
}

impl RankBias {
    pub const ALL: [RankBias; 3] = [
        RankBias::Fastest,
        RankBias::LeastWalking,
        RankBias::LatestDeparture,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            RankBias::Fastest => "最快",
            RankBias::LeastWalking => "少步行",
            RankBias::LatestDeparture => "更晚出发",
            RankBias::FewestTransfers => "少换乘",
        }
    }
}

/// 凭据类型 → UI 徽标
pub fn leg_label(kind: LegKind) -> &'static str {
    match kind {
        LegKind::Walk => "步行",
        LegKind::Bus => "乘车",
        LegKind::Wait => "候车",
        LegKind::Transfer => "换乘",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_bias_has_labels() {
        for b in RankBias::ALL {
            assert!(!b.label().is_empty());
        }
    }

    #[test]
    fn leg_labels_cover_all_kinds() {
        for k in [
            LegKind::Walk,
            LegKind::Bus,
            LegKind::Wait,
            LegKind::Transfer,
        ] {
            assert!(!leg_label(k).is_empty());
        }
    }
}
