#[cfg(test)]
mod tests {
    use chrono::Utc;
    use cloud_disk_sync::report::{SyncReport, SyncStatistics, SyncStatus};
    use cloud_disk_sync::sync::engine::SyncEngine;

    #[tokio::test]
    async fn test_report_persistence() {
        let engine = SyncEngine::new().await.unwrap();

        let task_id = format!("test_task_{}", uuid::Uuid::new_v4());
        let start_time = Utc::now();

        let mut report = SyncReport {
            task_id: task_id.clone(),
            start_time,
            end_time: Some(start_time + chrono::Duration::seconds(10)),
            status: SyncStatus::Success,
            statistics: SyncStatistics::default(),
            files: vec![],
            errors: vec![],
            warnings: vec![],
            duration_seconds: 10,
        };

        // Save report
        engine.save_report(&report).unwrap();

        // List reports
        let reports = engine.list_reports(&task_id, 10, 0).unwrap();
        assert_eq!(reports.len(), 1);
        let (id, time, status, duration) = &reports[0];
        assert_eq!(time, &start_time.timestamp());
        assert_eq!(status, "Success");
        assert_eq!(*duration, 10);

        // Get details
        let loaded_report = engine.get_report(id).unwrap();
        assert_eq!(loaded_report.task_id, task_id);
        assert_eq!(loaded_report.status, SyncStatus::Success);
    }
}
