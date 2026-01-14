use super::traits::RateLimiter;
use crate::error::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// 令牌桶算法实现
pub struct TokenBucketRateLimiter {
    capacity: u64,
    tokens: AtomicU64,
    refill_rate: f64, // tokens per second
    last_refill: parking_lot::Mutex<Instant>,
    semaphore: Arc<Semaphore>,
}

impl TokenBucketRateLimiter {
    pub fn new(capacity: u64, requests_per_second: f64) -> Self {
        Self {
            capacity,
            tokens: AtomicU64::new(capacity),
            refill_rate: requests_per_second,
            last_refill: parking_lot::Mutex::new(Instant::now()),
            semaphore: Arc::new(Semaphore::new(capacity as usize)),
        }
    }

    fn refill_tokens(&self) {
        let mut last_refill = self.last_refill.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill);

        if elapsed.as_secs_f64() > 0.0 {
            let new_tokens = (elapsed.as_secs_f64() * self.refill_rate) as u64;
            if new_tokens > 0 {
                let current = self.tokens.load(Ordering::Relaxed);
                let new_total = (current + new_tokens).min(self.capacity);
                self.tokens.store(new_total, Ordering::Relaxed);
                *last_refill = now;
            }
        }
    }
}

#[async_trait]
impl RateLimiter for TokenBucketRateLimiter {
    async fn acquire(&self) -> Result<()> {
        self.refill_tokens();

        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current == 0 {
                tokio::time::sleep(Duration::from_secs_f64(1.0 / self.refill_rate)).await;
                self.refill_tokens();
                continue;
            }

            if self.tokens.compare_exchange(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }

        Ok(())
    }

    fn current_rate(&self) -> f64 {
        self.refill_rate
    }

    fn set_rate(&self, requests_per_second: f64) {
        // 需要实现原子的更新
        // 这里简化处理
        unsafe {
            let self_mut = &mut *(self as *const Self as *mut Self);
            self_mut.refill_rate = requests_per_second;
        }
    }

    fn try_acquire(&self) -> bool {
        self.refill_tokens();

        let current = self.tokens.load(Ordering::Relaxed);
        if current == 0 {
            return false;
        }

        self.tokens.compare_exchange(
            current,
            current - 1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok()
    }
}

/// 滑动窗口限流器
pub struct SlidingWindowRateLimiter {
    pub(crate) window_size: Duration,
    pub(crate) max_requests: u64,
    pub(crate) requests: Mutex<Vec<Instant>>,
}

impl SlidingWindowRateLimiter {
    pub fn new(window_size: Duration, max_requests: u64) -> Self {
        Self {
            window_size,
            max_requests,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn cleanup_old_requests(&self) {
        let mut requests = self.requests.lock();
        let cutoff = Instant::now() - self.window_size;
        requests.unwrap().retain(|&time| time > cutoff);
    }
}

#[async_trait]
impl RateLimiter for SlidingWindowRateLimiter {
    async fn acquire<'a>(&'a self) -> Result<()>
    where
        Self: 'a,
    {
        loop {
            self.cleanup_old_requests();

            let requests = self.requests.lock();
            if requests.len() < self.max_requests as usize {
                drop(requests);
                break;
            }

            // 计算需要等待的时间
            let oldest = requests.first().unwrap();
            let wait_time = self.window_size - oldest.elapsed();
            if wait_time > Duration::from_secs(0) {
                tokio::time::sleep(wait_time).await;
            }
        }

        // 添加当前请求
        let mut requests = self.requests.lock();
        requests.push(Instant::now());
        Ok(())
    }

    fn current_rate(&self) -> f64 {
        self.cleanup_old_requests();
        let requests = self.requests.lock();
        requests.len() as f64 / self.window_size.as_secs_f64()
    }

    fn set_rate(&self, requests_per_second: f64) {
        // 调整窗口大小或最大请求数
        // 这里简化处理
    }

    fn try_acquire(&self) -> bool {
        self.cleanup_old_requests();

        let mut requests = self.requests.lock();
        if requests.len() < self.max_requests as usize {
            requests.push(Instant::now());
            true
        } else {
            false
        }
    }
}