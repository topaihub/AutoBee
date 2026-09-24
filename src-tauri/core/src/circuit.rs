//! Provider 熔断。
//!
//! # 为什么需要
//!
//! 一家厂商挂了（Key 过期、配额打满、机房故障）时，每次算路都要等它超时
//! 才降级到下一家 —— 用户看到的是「转圈十秒然后才出结果」。连续失败到阈值就
//! 直接跳过它，把等待时间还给用户。
//!
//! # 为什么计数放在进程内
//!
//! 桌面端只有一个进程，状态天然共享。放在 SQLite 里要处理过期清理与并发写，
//! 换来的只是「重启后还记得」，而重启本来就该重新试一次 —— 那家可能已经活了。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::models::ProviderId;

/// 连续失败几次就熔断。
const FAIL_THRESHOLD: u32 = 3;
/// 熔断多久。
const OPEN_FOR: Duration = Duration::from_secs(10 * 60);

/// 一家的状态。
#[derive(Debug, Clone, Copy, Default)]
struct State {
    /// 连续失败次数。
    fails: u32,
    /// 熔断到什么时候。
    open_until: Option<Instant>,
}

/// 熔断器。内部可变，可跨线程共享。
#[derive(Debug, Default)]
pub struct CircuitBreaker {
    states: Mutex<HashMap<ProviderId, State>>,
}

impl CircuitBreaker {
    /// 新建一个熔断器。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 这家是不是正在熔断中。
    ///
    /// 锁被别的线程污染（`poisoned`）时当「没熔断」：这时宁可多发一次请求，
    /// 也不要因为一次 panic 让某家永久不可用。
    #[must_use]
    pub fn is_open(&self, id: ProviderId) -> bool {
        let Ok(guard) = self.states.lock() else {
            return false;
        };
        guard
            .get(&id)
            .and_then(|s| s.open_until)
            .is_some_and(|until| until > Instant::now())
    }

    /// 记一次成功，清零。
    pub fn record_success(&self, id: ProviderId) {
        if let Ok(mut guard) = self.states.lock() {
            guard.insert(id, State::default());
        }
    }

    /// 记一次失败。到阈值就熔断。
    pub fn record_failure(&self, id: ProviderId) {
        if let Ok(mut guard) = self.states.lock() {
            let state = guard.entry(id).or_default();
            state.fails += 1;
            if state.fails >= FAIL_THRESHOLD {
                state.open_until = Some(Instant::now() + OPEN_FOR);
                tracing::warn!(provider = id.as_str(), "连续失败达到阈值，暂时熔断");
            }
        }
    }

    /// 当前状态，测试用。
    #[must_use]
    pub fn fails_of(&self, id: ProviderId) -> u32 {
        self.states
            .lock()
            .map(|g| g.get(&id).map_or(0, |s| s.fails))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 连续三次失败后熔断() {
        let cb = CircuitBreaker::new();
        assert!(!cb.is_open(ProviderId::Tencent));
        cb.record_failure(ProviderId::Tencent);
        cb.record_failure(ProviderId::Tencent);
        assert!(!cb.is_open(ProviderId::Tencent), "两次还不够");
        cb.record_failure(ProviderId::Tencent);
        assert!(cb.is_open(ProviderId::Tencent));
    }

    #[test]
    fn 成功后清零_熔断解除() {
        let cb = CircuitBreaker::new();
        for _ in 0..3 {
            cb.record_failure(ProviderId::Tencent);
        }
        assert!(cb.is_open(ProviderId::Tencent));
        cb.record_success(ProviderId::Tencent);
        assert!(!cb.is_open(ProviderId::Tencent));
        assert_eq!(cb.fails_of(ProviderId::Tencent), 0);
    }

    #[test]
    fn 各家互不干扰() {
        let cb = CircuitBreaker::new();
        for _ in 0..3 {
            cb.record_failure(ProviderId::Tencent);
        }
        assert!(cb.is_open(ProviderId::Tencent));
        assert!(!cb.is_open(ProviderId::Amap), "一家挂了不该连累别家");
    }

    #[test]
    fn 未记录过的厂商没熔断() {
        assert!(!CircuitBreaker::new().is_open(ProviderId::Here));
    }
}
