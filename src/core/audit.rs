use super::traits::AuditLogger;
use crate::error::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditOperation {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub operation_type: OperationType,
    pub user: String,
    pub resource: String,
    pub details: serde_json::Value,
    pub success: bool,
    pub error_message: Option<String>,
    pub duration_ms: u64,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    AccountAdd,
    AccountUpdate,
    AccountDelete,
    TaskCreate,
    TaskUpdate,
    TaskDelete,
    TaskRun,
    FileUpload,
    FileDownload,
    FileDelete,
    EncryptionKeyCreate,
    EncryptionKeyDelete,
    ConfigUpdate,
    HealthCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFilter {
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub operation_type: Option<OperationType>,
    pub user: Option<String>,
    pub resource: Option<String>,
    pub success: Option<bool>,
    pub min_duration_ms: Option<u64>,
    pub max_duration_ms: Option<u64>,
}

pub struct DatabaseAuditLogger {
    connection: std::sync::Mutex<Connection>,
}

impl DatabaseAuditLogger {
    pub fn new(db_path: &std::path::Path) -> Result<Self> {
        let connection = Connection::open(db_path)?;

        // 创建审计表
        connection.execute(
            r#"
            CREATE TABLE IF NOT EXISTS audit_logs (
                id TEXT PRIMARY KEY,
                timestamp DATETIME NOT NULL,
                operation_type TEXT NOT NULL,
                user TEXT NOT NULL,
                resource TEXT NOT NULL,
                details TEXT NOT NULL,
                success BOOLEAN NOT NULL,
                error_message TEXT,
                duration_ms INTEGER NOT NULL,
                ip_address TEXT,
                user_agent TEXT
            )
            "#,
            [],
        )?;

        // 创建索引
        connection.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp)",
            [],
        )?;

        connection.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_operation ON audit_logs(operation_type)",
            [],
        )?;

        connection.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_logs(user)",
            [],
        )?;

        Ok(Self {
            connection: std::sync::Mutex::new(connection),
        })
    }
}

impl AuditLogger for DatabaseAuditLogger {
    fn log_operation(&self, operation: AuditOperation) {
        let conn = self.connection.lock().unwrap();
        let _ = conn.execute(
            r#"
            INSERT INTO audit_logs
            (id, timestamp, operation_type, user, resource, details, success, error_message, duration_ms, ip_address, user_agent)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            rusqlite::params![
                operation.id,
                operation.timestamp.to_rfc3339(),
                serde_json::to_string(&operation.operation_type).unwrap(),
                operation.user,
                operation.resource,
                serde_json::to_string(&operation.details).unwrap(),
                operation.success,
                operation.error_message,
                operation.duration_ms as i64,
                operation.ip_address,
                operation.user_agent,
            ],
        );
    }

    fn query_operations(
        &self,
        filter: AuditFilter,
        limit: Option<usize>,
    ) -> Result<Vec<AuditOperation>> {
        let mut query = "SELECT * FROM audit_logs WHERE 1=1".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start_time) = filter.start_time {
            query.push_str(" AND timestamp >= ?");
            params.push(Box::new(start_time.to_rfc3339()));
        }

        if let Some(end_time) = filter.end_time {
            query.push_str(" AND timestamp <= ?");
            params.push(Box::new(end_time.to_rfc3339()));
        }

        if let Some(op_type) = filter.operation_type {
            query.push_str(" AND operation_type = ?");
            params.push(Box::new(serde_json::to_string(&op_type)?));
        }

        if let Some(user) = filter.user {
            query.push_str(" AND user = ?");
            params.push(Box::new(user));
        }

        if let Some(resource) = filter.resource {
            query.push_str(" AND resource = ?");
            params.push(Box::new(resource));
        }

        if let Some(success) = filter.success {
            query.push_str(" AND success = ?");
            params.push(Box::new(success));
        }

        if let Some(min_duration) = filter.min_duration_ms {
            query.push_str(" AND duration_ms >= ?");
            params.push(Box::new(min_duration as i64));
        }

        if let Some(max_duration) = filter.max_duration_ms {
            query.push_str(" AND duration_ms <= ?");
            params.push(Box::new(max_duration as i64));
        }

        query.push_str(" ORDER BY timestamp DESC");

        if let Some(limit) = limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }

        let conn = self.connection.lock().unwrap();
        let mut stmt = conn.prepare(&query)?;

        let mut rows = stmt.query(rusqlite::params_from_iter(params.into_iter()))?;
        let mut operations = Vec::new();

        while let Some(row) = rows.next()? {
            let operation = AuditOperation {
                id: row.get(0)?,
                timestamp: {
                    let ts: String = row.get(1)?;
                    chrono::DateTime::parse_from_rfc3339(&ts)
                        .map_err(|e| crate::error::SyncError::Validation(e.to_string()))?
                        .with_timezone(&Utc)
                },
                operation_type: serde_json::from_str(&row.get::<_, String>(2)?)?,
                user: row.get(3)?,
                resource: row.get(4)?,
                details: serde_json::from_str(&row.get::<_, String>(5)?)?,
                success: row.get(6)?,
                error_message: row.get(7)?,
                duration_ms: {
                    let v: i64 = row.get(8)?;
                    v as u64
                },
                ip_address: row.get(9)?,
                user_agent: row.get(10)?,
            };

            operations.push(operation);
        }

        Ok(operations)
    }
}
