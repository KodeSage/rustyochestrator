use rustyochestrator::cache::{Cache, CacheEntry};

#[test]
fn test_default_cache_is_empty() {
    let cache = Cache::default();
    assert!(cache.entries.is_empty());
}

#[test]
fn test_is_hit_unknown_task_returns_false() {
    let cache = Cache::default();
    assert!(!cache.is_hit("unknown", "anyhash"));
}

#[test]
fn test_is_hit_wrong_hash_returns_false() {
    let mut cache = Cache::default();
    cache.record("task1".to_string(), "correct_hash".to_string(), true);
    assert!(!cache.is_hit("task1", "wrong_hash"));
}

#[test]
fn test_is_hit_failed_task_with_matching_hash_returns_false() {
    let mut cache = Cache::default();
    cache.record("task1".to_string(), "hash123".to_string(), false);
    assert!(!cache.is_hit("task1", "hash123"));
}

#[test]
fn test_is_hit_successful_task_with_matching_hash_returns_true() {
    let mut cache = Cache::default();
    cache.record("task1".to_string(), "hash123".to_string(), true);
    assert!(cache.is_hit("task1", "hash123"));
}

#[test]
fn test_record_overwrites_failed_with_success() {
    let mut cache = Cache::default();
    cache.record("task1".to_string(), "hash123".to_string(), false);
    assert!(!cache.is_hit("task1", "hash123"));
    cache.record("task1".to_string(), "hash123".to_string(), true);
    assert!(cache.is_hit("task1", "hash123"));
}

#[test]
fn test_record_overwrites_hash() {
    let mut cache = Cache::default();
    cache.record("task1".to_string(), "old_hash".to_string(), true);
    cache.record("task1".to_string(), "new_hash".to_string(), true);
    assert!(!cache.is_hit("task1", "old_hash"));
    assert!(cache.is_hit("task1", "new_hash"));
}

#[test]
fn test_record_multiple_tasks() {
    let mut cache = Cache::default();
    cache.record("task1".to_string(), "hash1".to_string(), true);
    cache.record("task2".to_string(), "hash2".to_string(), true);
    cache.record("task3".to_string(), "hash3".to_string(), false);

    assert!(cache.is_hit("task1", "hash1"));
    assert!(cache.is_hit("task2", "hash2"));
    assert!(!cache.is_hit("task3", "hash3"));
    assert_eq!(cache.entries.len(), 3);
}

#[test]
fn test_cache_json_round_trip() {
    let mut cache = Cache::default();
    cache.record("build".to_string(), "abc123".to_string(), true);
    cache.record("test".to_string(), "def456".to_string(), false);

    let json = serde_json::to_string(&cache).unwrap();
    let restored: Cache = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.entries.len(), 2);
    assert!(restored.is_hit("build", "abc123"));
    assert!(!restored.is_hit("test", "def456"));
}

#[test]
fn test_cache_entry_fields() {
    let entry = CacheEntry {
        hash: "abc123".to_string(),
        success: true,
    };
    assert_eq!(entry.hash, "abc123");
    assert!(entry.success);
}

#[test]
fn test_cache_entry_failed() {
    let entry = CacheEntry {
        hash: "abc123".to_string(),
        success: false,
    };
    assert!(!entry.success);
}

#[test]
fn test_is_hit_prefix_of_hash_is_not_a_match() {
    let mut cache = Cache::default();
    let full_hash = "a".repeat(64);
    cache.record("task".to_string(), full_hash.clone(), true);
    let prefix = &full_hash[..32];
    assert!(!cache.is_hit("task", prefix));
}

#[test]
fn test_cache_json_contains_entries_key() {
    let cache = Cache::default();
    let json = serde_json::to_string(&cache).unwrap();
    assert!(json.contains("entries"));
}
