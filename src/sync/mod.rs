pub mod engine;
pub mod diff;

pub struct VerificationResult {
    pub total_files: i32,
    pub passed: i32,
    pub failed: i32,
    pub skipped: i32,
    pub errors: Vec<ErrorResult>,
}

struct ErrorResult {}
