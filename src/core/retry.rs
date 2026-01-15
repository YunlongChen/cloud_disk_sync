use super::traits::RetryStrategy;
use crate::error::SyncError;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ExponentialBackoffRetry {
    max_attempts: u32,
    initial_delay: Duration,
    max_delay: Duration,
    backoff_factor: f64,
    retryable_errors: Vec<u32>,
}

impl ExponentialBackoffRetry {
    pub fn new() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_factor: 2.0,
            retryable_errors: vec![
                5000,  // Io errors
                6000,  // Network errors
                11000, // Timeout
                12000, // Rate limit
                16000, // Resource exhausted
            ],
        }
    }

    pub fn builder() -> ExponentialBackoffRetryBuilder {
        ExponentialBackoffRetryBuilder::new()
    }
}

impl RetryStrategy for ExponentialBackoffRetry {
    fn should_retry(&self, attempt: u32, error: &SyncError) -> bool {
        if attempt >= self.max_attempts {
            return false;
        }

        // 检查错误是否可重试
        if error.is_retryable() {
            return true;
        }

        // 检查错误码是否在可重试列表中
        self.retryable_errors.contains(&error.error_code())
    }

    fn delay_before_retry(&self, attempt: u32) -> Duration {
        let delay = self.initial_delay.as_secs_f64() * self.backoff_factor.powi(attempt as i32);

        Duration::from_secs_f64(delay.min(self.max_delay.as_secs_f64()))
    }

    fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
}

// 构建器模式
pub struct ExponentialBackoffRetryBuilder {
    max_attempts: Option<u32>,
    initial_delay: Option<Duration>,
    max_delay: Option<Duration>,
    backoff_factor: Option<f64>,
    retryable_errors: Vec<u32>,
}

impl ExponentialBackoffRetryBuilder {
    pub fn new() -> Self {
        Self {
            max_attempts: None,
            initial_delay: None,
            max_delay: None,
            backoff_factor: None,
            retryable_errors: Vec::new(),
        }
    }

    pub fn max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = Some(attempts);
        self
    }

    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = Some(delay);
        self
    }

    pub fn max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = Some(delay);
        self
    }

    pub fn backoff_factor(mut self, factor: f64) -> Self {
        self.backoff_factor = Some(factor);
        self
    }

    pub fn retryable_error(mut self, error_code: u32) -> Self {
        self.retryable_errors.push(error_code);
        self
    }

    pub fn build(self) -> ExponentialBackoffRetry {
        ExponentialBackoffRetry {
            max_attempts: self.max_attempts.unwrap_or(5),
            initial_delay: self.initial_delay.unwrap_or(Duration::from_secs(1)),
            max_delay: self.max_delay.unwrap_or(Duration::from_secs(60)),
            backoff_factor: self.backoff_factor.unwrap_or(2.0),
            retryable_errors: self.retryable_errors,
        }
    }
}

// Jitter重试策略
pub struct JitterRetry {
    base: ExponentialBackoffRetry,
    jitter_ratio: f64,
}

impl JitterRetry {
    pub fn new() -> Self {
        Self {
            base: ExponentialBackoffRetry::new(),
            jitter_ratio: 0.1,
        }
    }

    fn add_jitter(&self, delay: Duration) -> Duration {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let jitter = rng.gen_range(-self.jitter_ratio..self.jitter_ratio);
        let secs = delay.as_secs_f64() * (1.0 + jitter);
        Duration::from_secs_f64(secs)
    }
}

impl RetryStrategy for JitterRetry {
    fn should_retry(&self, attempt: u32, error: &SyncError) -> bool {
        self.base.should_retry(attempt, error)
    }

    fn delay_before_retry(&self, attempt: u32) -> Duration {
        let delay = self.base.delay_before_retry(attempt);
        self.add_jitter(delay)
    }

    fn max_attempts(&self) -> u32 {
        self.base.max_attempts()
    }
}
