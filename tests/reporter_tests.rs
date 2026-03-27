use rustyochestrator::reporter::{Event, PipelineCompletedArgs, Reporter};

#[test]
fn test_pipeline_started_event_fields() {
    let event = Event::pipeline_started("pipe-abc", "my-pipeline", 7, "alice");
    match event {
        Event::PipelineStarted {
            pipeline_id,
            pipeline_name,
            total_tasks,
            user_login,
            started_at,
        } => {
            assert_eq!(pipeline_id, "pipe-abc");
            assert_eq!(pipeline_name, "my-pipeline");
            assert_eq!(total_tasks, 7);
            assert_eq!(user_login, "alice");
            assert!(!started_at.is_empty());
        }
        _ => panic!("expected PipelineStarted"),
    }
}

#[test]
fn test_task_completed_event_success() {
    let event = Event::task_completed("pipe-abc", "build", false, 1500, true);
    match event {
        Event::TaskCompleted {
            pipeline_id,
            task_id,
            cache_hit,
            duration_ms,
            success,
        } => {
            assert_eq!(pipeline_id, "pipe-abc");
            assert_eq!(task_id, "build");
            assert!(!cache_hit);
            assert_eq!(duration_ms, 1500);
            assert!(success);
        }
        _ => panic!("expected TaskCompleted"),
    }
}

#[test]
fn test_task_completed_event_cache_hit() {
    let event = Event::task_completed("pipe-abc", "build", true, 0, true);
    match event {
        Event::TaskCompleted {
            cache_hit,
            duration_ms,
            success,
            ..
        } => {
            assert!(cache_hit);
            assert_eq!(duration_ms, 0);
            assert!(success);
        }
        _ => panic!("expected TaskCompleted"),
    }
}

#[test]
fn test_task_completed_event_failure() {
    let event = Event::task_completed("pipe-abc", "test", false, 500, false);
    match event {
        Event::TaskCompleted { success, .. } => {
            assert!(!success);
        }
        _ => panic!("expected TaskCompleted"),
    }
}

#[test]
fn test_pipeline_completed_success_status() {
    let event = Event::pipeline_completed(PipelineCompletedArgs {
        id: "pipe-123",
        name: "ci",
        success: true,
        total_tasks: 5,
        cached_tasks: 2,
        failed_tasks: 0,
        duration_ms: 12000,
        user_login: "alice",
    });
    match event {
        Event::PipelineCompleted {
            pipeline_id,
            pipeline_name,
            status,
            total_tasks,
            cached_tasks,
            failed_tasks,
            duration_ms,
            user_login,
            finished_at,
        } => {
            assert_eq!(pipeline_id, "pipe-123");
            assert_eq!(pipeline_name, "ci");
            assert_eq!(status, "success");
            assert_eq!(total_tasks, 5);
            assert_eq!(cached_tasks, 2);
            assert_eq!(failed_tasks, 0);
            assert_eq!(duration_ms, 12000);
            assert_eq!(user_login, "alice");
            assert!(!finished_at.is_empty());
        }
        _ => panic!("expected PipelineCompleted"),
    }
}

#[test]
fn test_pipeline_completed_failed_status() {
    let event = Event::pipeline_completed(PipelineCompletedArgs {
        id: "pipe-456",
        name: "failing",
        success: false,
        total_tasks: 3,
        cached_tasks: 0,
        failed_tasks: 2,
        duration_ms: 3000,
        user_login: "bob",
    });
    match event {
        Event::PipelineCompleted {
            status,
            failed_tasks,
            ..
        } => {
            assert_eq!(status, "failed");
            assert_eq!(failed_tasks, 2);
        }
        _ => panic!("expected PipelineCompleted"),
    }
}

#[test]
fn test_pipeline_started_serializes_with_type_tag() {
    let event = Event::pipeline_started("id", "name", 1, "user");
    let json = serde_json::to_string(&event).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["type"], "pipeline_started");
}

#[test]
fn test_task_completed_serializes_with_type_tag() {
    let event = Event::task_completed("id", "task", false, 100, true);
    let json = serde_json::to_string(&event).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["type"], "task_completed");
}

#[test]
fn test_pipeline_completed_serializes_with_type_tag() {
    let event = Event::pipeline_completed(PipelineCompletedArgs {
        id: "id",
        name: "name",
        success: true,
        total_tasks: 1,
        cached_tasks: 0,
        failed_tasks: 0,
        duration_ms: 100,
        user_login: "user",
    });
    let json = serde_json::to_string(&event).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["type"], "pipeline_completed");
}

#[test]
fn test_started_at_iso8601_format() {
    let event = Event::pipeline_started("id", "name", 1, "user");
    if let Event::PipelineStarted { started_at, .. } = event {
        // Format: YYYY-MM-DDTHH:MM:SSZ (length 20)
        assert_eq!(started_at.len(), 20, "got: {}", started_at);
        assert!(started_at.ends_with('Z'), "got: {}", started_at);
        assert!(started_at.contains('T'), "got: {}", started_at);
        assert_eq!(&started_at[4..5], "-");
        assert_eq!(&started_at[7..8], "-");
        assert_eq!(&started_at[13..14], ":");
        assert_eq!(&started_at[16..17], ":");
    }
}

#[test]
fn test_reporter_can_be_constructed() {
    let _reporter = Reporter::new("https://example.com".to_string(), "test-token".to_string());
}

#[test]
fn test_pipeline_started_json_has_all_required_fields() {
    let event = Event::pipeline_started("pipe-1", "my-pipe", 3, "alice");
    let json = serde_json::to_string(&event).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.get("pipeline_id").is_some());
    assert!(val.get("pipeline_name").is_some());
    assert!(val.get("total_tasks").is_some());
    assert!(val.get("started_at").is_some());
    assert!(val.get("user_login").is_some());
}
