use crate::error::{Result, SyncError};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// 内存句柄 - 表示已分配的内存区域
#[derive(Debug, Clone)]
pub struct MemoryHandle {
    id: String,
    size: usize,
    allocated_at: Instant,
    // 可以添加更多字段，如内存地址、使用统计等
}

impl MemoryHandle {
    pub fn new(id: String, size: usize) -> Self {
        Self {
            id,
            size,
            allocated_at: Instant::now(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn allocated_at(&self) -> Instant {
        self.allocated_at
    }

    pub fn age(&self) -> Duration {
        self.allocated_at.elapsed()
    }
}

/// 磁盘句柄 - 表示已分配的磁盘空间
#[derive(Debug, Clone)]
pub struct DiskHandle {
    id: String,
    path: std::path::PathBuf,
    size: u64,
    allocated_at: Instant,
    is_temporary: bool,
}

impl DiskHandle {
    pub fn new(id: String, path: std::path::PathBuf, size: u64, is_temporary: bool) -> Self {
        Self {
            id,
            path,
            size,
            allocated_at: Instant::now(),
            is_temporary,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn is_temporary(&self) -> bool {
        self.is_temporary
    }

    pub fn allocated_at(&self) -> Instant {
        self.allocated_at
    }

    pub fn age(&self) -> Duration {
        self.allocated_at.elapsed()
    }

    /// 清理磁盘空间
    pub fn cleanup(&self) -> Result<()> {
        if self.is_temporary && self.path.exists() {
            if self.path.is_dir() {
                std::fs::remove_dir_all(&self.path)?;
            } else {
                std::fs::remove_file(&self.path)?;
            }
        }
        Ok(())
    }
}

/// 资源使用情况
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    pub memory_used: usize,
    pub memory_limit: Option<usize>,
    pub memory_percentage: f64,

    pub disk_used: u64,
    pub disk_limit: Option<u64>,
    pub disk_percentage: f64,

    pub file_descriptors_used: usize,
    pub file_descriptors_limit: Option<usize>,
    pub file_descriptors_percentage: f64,

    pub cpu_usage: f64,                       // 百分比
    pub network_bandwidth_used: u64,          // bytes/sec
    pub network_bandwidth_limit: Option<u64>, // bytes/sec

    pub active_tasks: usize,
    pub max_concurrent_tasks: usize,

    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl ResourceUsage {
    pub fn new() -> Self {
        Self {
            memory_used: 0,
            memory_limit: None,
            memory_percentage: 0.0,

            disk_used: 0,
            disk_limit: None,
            disk_percentage: 0.0,

            file_descriptors_used: 0,
            file_descriptors_limit: None,
            file_descriptors_percentage: 0.0,

            cpu_usage: 0.0,
            network_bandwidth_used: 0,
            network_bandwidth_limit: None,

            active_tasks: 0,
            max_concurrent_tasks: 0,

            last_updated: chrono::Utc::now(),
        }
    }

    pub fn update_from_system(&mut self) -> Result<()> {
        // 获取系统内存信息
        let sys_info_result = sys_info::mem_info();

        if let Ok(mem_info) = sys_info_result {
            self.memory_used = mem_info.total as usize - mem_info.free as usize;
            self.memory_limit = Some(mem_info.total as usize);
            self.memory_percentage = (self.memory_used as f64 / mem_info.total as f64) * 100.0;
        }

        // 获取磁盘使用情况
        if let Some(home_dir) = dirs::data_dir()
            && let Ok(disk_info) = fs2::available_space(&home_dir)
        {
            // 这里需要计算已使用空间，但fs2只提供可用空间
            // 实际中可能需要其他方式获取
            self.disk_used = 0; // 简化处理
            self.disk_limit = Some(disk_info);
            self.disk_percentage = 0.0;
        }

        // 获取文件描述符数量（仅限Unix）
        #[cfg(unix)]
        {
            use std::fs;
            let fd_dir = "/proc/self/fd";
            if let Ok(entries) = fs::read_dir(fd_dir) {
                self.file_descriptors_used = entries.count();
                // 获取限制
                if let Ok(limit) = rlimit::getrlimit(rlimit::Resource::NOFILE) {
                    self.file_descriptors_limit = Some(limit.1 as usize);
                    self.file_descriptors_percentage =
                        (self.file_descriptors_used as f64 / limit.1 as f64) * 100.0;
                }
            }
        }

        // 获取CPU使用率（需要系统特定代码）
        // 简化处理

        self.last_updated = chrono::Utc::now();
        Ok(())
    }

    pub fn is_overloaded(&self) -> bool {
        // 检查是否超过任何限制
        if let Some(_limit) = self.memory_limit
            && self.memory_percentage > 90.0
        {
            return true;
        }

        if let Some(_limit) = self.disk_limit
            && self.disk_percentage > 90.0
        {
            return true;
        }

        if let Some(_limit) = self.file_descriptors_limit
            && self.file_descriptors_percentage > 90.0
        {
            return true;
        }

        if self.cpu_usage > 90.0 {
            return true;
        }

        false
    }

    pub fn to_string(&self) -> String {
        format!(
            "Memory: {:.1}% used, Disk: {:.1}% used, CPU: {:.1}%, Tasks: {}/{}",
            self.memory_percentage,
            self.disk_percentage,
            self.cpu_usage,
            self.active_tasks,
            self.max_concurrent_tasks
        )
    }
}

/// 资源限制
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResourceLimits {
    pub max_memory_bytes: Option<usize>,
    pub max_disk_bytes: Option<u64>,
    pub max_file_descriptors: Option<usize>,
    pub max_cpu_percentage: Option<f64>,
    pub max_network_bandwidth_bytes_per_sec: Option<u64>,
    pub max_concurrent_tasks: usize,
    pub max_file_size_bytes: Option<u64>,
    pub max_total_files: Option<usize>,
    pub max_retention_days: Option<u32>,
}

impl ResourceLimits {
    pub fn default() -> Self {
        Self {
            max_memory_bytes: Some(1024 * 1024 * 1024),    // 1GB
            max_disk_bytes: Some(10 * 1024 * 1024 * 1024), // 10GB
            max_file_descriptors: Some(1024),
            max_cpu_percentage: Some(80.0),
            max_network_bandwidth_bytes_per_sec: Some(10 * 1024 * 1024), // 10MB/s
            max_concurrent_tasks: 10,
            max_file_size_bytes: Some(5 * 1024 * 1024 * 1024), // 5GB
            max_total_files: Some(100_000),
            max_retention_days: Some(365),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(limit) = self.max_memory_bytes
            && limit < 1024 * 1024
        {
            // 小于1MB
            return Err(SyncError::Validation(
                "Memory limit too low (minimum 1MB)".into(),
            ));
        }

        if let Some(limit) = self.max_disk_bytes
            && limit < 1024 * 1024
        {
            // 小于1MB
            return Err(SyncError::Validation(
                "Disk limit too low (minimum 1MB)".into(),
            ));
        }

        if self.max_concurrent_tasks == 0 {
            return Err(SyncError::Validation(
                "Max concurrent tasks must be greater than 0".into(),
            ));
        }

        Ok(())
    }
}

/// 资源管理器实现
pub struct ResourceManagerImpl {
    limits: Mutex<ResourceLimits>,
    current_usage: Mutex<ResourceUsage>,
    allocated_memory: Arc<AtomicUsize>,
    allocated_disk: Arc<AtomicU64>,
    active_tasks: Arc<AtomicUsize>,
    memory_handles: Mutex<HashMap<String, MemoryHandle>>,
    disk_handles: Mutex<HashMap<String, DiskHandle>>,
}

impl ResourceManagerImpl {
    pub fn new(limits: ResourceLimits) -> Self {
        limits.validate().expect("Invalid resource limits");

        Self {
            limits: Mutex::new(limits),
            current_usage: Mutex::new(ResourceUsage::new()),
            allocated_memory: Arc::new(AtomicUsize::new(0)),
            allocated_disk: Arc::new(AtomicU64::new(0)),
            active_tasks: Arc::new(AtomicUsize::new(0)),
            memory_handles: Mutex::new(HashMap::new()),
            disk_handles: Mutex::new(HashMap::new()),
        }
    }

    pub fn allocate_memory(&self, size: usize) -> Result<MemoryHandle> {
        let limits = self.limits.lock();

        // 检查内存限制
        if let Some(max_memory) = limits.max_memory_bytes {
            let current = self.allocated_memory.load(Ordering::Relaxed);
            if current + size > max_memory {
                return Err(SyncError::ResourceExhausted(format!(
                    "Memory limit exceeded: {} > {}",
                    current + size,
                    max_memory
                )));
            }
        }

        // 生成唯一ID
        let id = format!("mem_{}", Uuid::new_v4());

        // 更新已分配内存
        self.allocated_memory.fetch_add(size, Ordering::SeqCst);

        // 创建内存句柄
        let handle = MemoryHandle::new(id.clone(), size);

        // 存储句柄
        let mut handles = self.memory_handles.lock();
        handles.insert(id, handle.clone());

        Ok(handle)
    }

    pub fn allocate_disk(&self, size: u64) -> Result<DiskHandle> {
        let limits = self.limits.lock();

        // 检查磁盘限制
        if let Some(max_disk) = limits.max_disk_bytes {
            let current = self.allocated_disk.load(Ordering::Relaxed);
            if current + size > max_disk {
                return Err(SyncError::ResourceExhausted(format!(
                    "Disk limit exceeded: {} > {}",
                    current + size,
                    max_disk
                )));
            }
        }

        // 创建临时文件或目录
        let temp_dir = std::env::temp_dir();

        let unique_name = Uuid::new_v4();
        let temp_path = temp_dir.join(format!("disksync_{}", unique_name));

        // 创建文件或预留空间
        std::fs::File::create(&temp_path)?;
        if size > 0 {
            // 预分配磁盘空间
            // 注意：这取决于操作系统和文件系统支持
            let file = std::fs::OpenOptions::new().write(true).open(&temp_path)?;
            file.set_len(size)?;
        }

        // 生成唯一ID
        let id = format!("disk_{}", unique_name);

        // 更新已分配磁盘空间
        self.allocated_disk.fetch_add(size, Ordering::SeqCst);

        // 创建磁盘句柄
        let handle = DiskHandle::new(id.clone(), temp_path, size, true);

        // 存储句柄
        let mut handles = self.disk_handles.lock();
        handles.insert(id, handle.clone());

        Ok(handle)
    }

    pub fn deallocate_memory(&self, handle_id: &str) -> Result<()> {
        let mut handles = self.memory_handles.lock();

        if let Some(handle) = handles.remove(handle_id) {
            // 更新已分配内存
            self.allocated_memory
                .fetch_sub(handle.size(), Ordering::SeqCst);
            Ok(())
        } else {
            Err(SyncError::Validation(format!(
                "Memory handle not found: {}",
                handle_id
            )))
        }
    }

    pub fn deallocate_disk(&self, handle_id: &str) -> Result<()> {
        let mut handles = self.disk_handles.lock();

        if let Some(handle) = handles.remove(handle_id) {
            // 清理磁盘空间
            handle.cleanup()?;

            // 更新已分配磁盘空间
            self.allocated_disk
                .fetch_sub(handle.size(), Ordering::SeqCst);

            Ok(())
        } else {
            Err(SyncError::Validation(format!(
                "Disk handle not found: {}",
                handle_id
            )))
        }
    }

    pub fn current_usage(&self) -> ResourceUsage {
        let mut usage = self.current_usage.lock().clone();

        // 更新动态字段
        usage.memory_used = self.allocated_memory.load(Ordering::Relaxed);
        usage.disk_used = self.allocated_disk.load(Ordering::Relaxed);
        usage.active_tasks = self.active_tasks.load(Ordering::Relaxed);

        // 设置限制
        let limits = self.limits.lock();
        usage.memory_limit = limits.max_memory_bytes;
        usage.disk_limit = limits.max_disk_bytes;
        usage.max_concurrent_tasks = limits.max_concurrent_tasks;
        usage.network_bandwidth_limit = limits.max_network_bandwidth_bytes_per_sec;

        // 计算百分比
        if let Some(limit) = usage.memory_limit {
            usage.memory_percentage = (usage.memory_used as f64 / limit as f64) * 100.0;
        }

        if let Some(limit) = usage.disk_limit {
            usage.disk_percentage = (usage.disk_used as f64 / limit as f64) * 100.0;
        }

        usage.last_updated = chrono::Utc::now();
        usage
    }

    pub fn set_limits(&self, limits: ResourceLimits) {
        limits.validate().expect("Invalid resource limits");
        *self.limits.lock() = limits;
    }

    pub fn start_task(&self) -> Result<()> {
        let current = self.active_tasks.fetch_add(1, Ordering::SeqCst);
        let limits = self.limits.lock();

        if current >= limits.max_concurrent_tasks {
            self.active_tasks.fetch_sub(1, Ordering::SeqCst);
            return Err(SyncError::ResourceExhausted(format!(
                "Concurrent task limit exceeded: {} >= {}",
                current + 1,
                limits.max_concurrent_tasks
            )));
        }

        Ok(())
    }

    pub fn end_task(&self) {
        self.active_tasks.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn cleanup_old_resources(&self, max_age: Duration) -> Result<()> {
        let _now = Instant::now();

        // 清理旧的内存句柄
        {
            let mut handles = self.memory_handles.lock();
            let old_handles: Vec<String> = handles
                .iter()
                .filter(|(_, handle)| handle.age() > max_age)
                .map(|(id, _)| id.clone())
                .collect();

            for id in old_handles {
                if let Some(handle) = handles.remove(&id) {
                    self.allocated_memory
                        .fetch_sub(handle.size(), Ordering::SeqCst);
                }
            }
        }

        // 清理旧的磁盘句柄
        {
            let mut handles = self.disk_handles.lock();
            let old_handles: Vec<String> = handles
                .iter()
                .filter(|(_, handle)| handle.age() > max_age && handle.is_temporary())
                .map(|(id, _)| id.clone())
                .collect();

            for id in old_handles {
                if let Some(handle) = handles.remove(&id) {
                    handle.cleanup()?;
                    self.allocated_disk
                        .fetch_sub(handle.size(), Ordering::SeqCst);
                }
            }
        }

        Ok(())
    }
}
