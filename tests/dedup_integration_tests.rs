// ABOUTME: Integration tests for event deduplication
// ABOUTME: Tests real async channel behavior and concurrent message handling patterns

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Simulates the EventDeduplicator behavior used in main.rs
/// This is a copy of the production struct for testing purposes
struct EventDeduplicator {
    seen_events: HashSet<String>,
    max_size: usize,
}

impl EventDeduplicator {
    fn new(max_size: usize) -> Self {
        Self {
            seen_events: HashSet::new(),
            max_size,
        }
    }

    fn check_and_mark(&mut self, event_id: &str) -> bool {
        if self.seen_events.contains(event_id) {
            return false;
        }

        if self.seen_events.len() >= self.max_size {
            self.seen_events.clear();
        }

        self.seen_events.insert(event_id.to_string());
        true
    }
}

/// Simulates a Matrix event with event_id and room_id
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct SimulatedEvent {
    event_id: String,
    room_id: String,
    content: String, // Included for realistic event simulation
}

impl SimulatedEvent {
    fn new(event_id: &str, room_id: &str, content: &str) -> Self {
        Self {
            event_id: event_id.to_string(),
            room_id: room_id.to_string(),
            content: content.to_string(),
        }
    }
}

// =============================================================================
// SCENARIO: Single message is processed exactly once
// =============================================================================
#[tokio::test]
async fn scenario_single_message_processed_once() {
    let (tx, mut rx) = mpsc::channel::<SimulatedEvent>(256);
    let mut dedup = EventDeduplicator::new(10000);
    let mut processed_count = 0;

    // Send a single message
    let event = SimulatedEvent::new(
        "$abc123:matrix.org",
        "!room1:matrix.org",
        "Hello world",
    );
    tx.send(event.clone()).await.unwrap();
    drop(tx); // Close channel

    // Process messages with deduplication
    while let Some(event) = rx.recv().await {
        if dedup.check_and_mark(&event.event_id) {
            processed_count += 1;
        }
    }

    assert_eq!(processed_count, 1, "Single message should be processed exactly once");
}

// =============================================================================
// SCENARIO: Duplicate events are rejected
// =============================================================================
#[tokio::test]
async fn scenario_duplicate_events_rejected() {
    let (tx, mut rx) = mpsc::channel::<SimulatedEvent>(256);
    let mut dedup = EventDeduplicator::new(10000);
    let mut processed_count = 0;
    let mut skipped_count = 0;

    // Send the same event 10 times (simulating the bug we observed)
    let event = SimulatedEvent::new(
        "$duplicate123:matrix.org",
        "!japan:matrix.org",
        "Test message",
    );

    for _ in 0..10 {
        tx.send(event.clone()).await.unwrap();
    }
    drop(tx);

    // Process with deduplication
    while let Some(event) = rx.recv().await {
        if dedup.check_and_mark(&event.event_id) {
            processed_count += 1;
        } else {
            skipped_count += 1;
        }
    }

    assert_eq!(processed_count, 1, "Duplicate event should be processed only once");
    assert_eq!(skipped_count, 9, "9 duplicates should be skipped");
}

// =============================================================================
// SCENARIO: Different events all processed
// =============================================================================
#[tokio::test]
async fn scenario_different_events_all_processed() {
    let (tx, mut rx) = mpsc::channel::<SimulatedEvent>(256);
    let mut dedup = EventDeduplicator::new(10000);
    let mut processed_events = Vec::new();

    // Send 5 different events
    for i in 0..5 {
        let event = SimulatedEvent::new(
            &format!("$event{}:matrix.org", i),
            "!room:matrix.org",
            &format!("Message {}", i),
        );
        tx.send(event).await.unwrap();
    }
    drop(tx);

    // Process with deduplication
    while let Some(event) = rx.recv().await {
        if dedup.check_and_mark(&event.event_id) {
            processed_events.push(event.event_id.clone());
        }
    }

    assert_eq!(processed_events.len(), 5, "All 5 unique events should be processed");
}

// =============================================================================
// SCENARIO: Burst of duplicates from multiple rooms (realistic production pattern)
// =============================================================================
#[tokio::test]
async fn scenario_burst_duplicates_multiple_rooms() {
    let (tx, mut rx) = mpsc::channel::<SimulatedEvent>(256);
    let mut dedup = EventDeduplicator::new(10000);
    let mut processed_by_room: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    // Simulate the burst pattern observed in production logs:
    // Multiple copies of events from different rooms arriving simultaneously
    let rooms = vec!["!japan:matrix.org", "!dev:matrix.org", "!test:matrix.org"];

    for room in &rooms {
        let event = SimulatedEvent::new(
            &format!("$event_{}:matrix.org", room.replace("!", "").replace(":", "_")),
            room,
            "Burst message",
        );
        // Each room's event arrives 10 times (the bug pattern)
        for _ in 0..10 {
            tx.send(event.clone()).await.unwrap();
        }
    }
    drop(tx);

    // Process with deduplication
    while let Some(event) = rx.recv().await {
        if dedup.check_and_mark(&event.event_id) {
            *processed_by_room.entry(event.room_id.clone()).or_insert(0) += 1;
        }
    }

    // Each room should have exactly 1 processed event
    for room in &rooms {
        let count = processed_by_room.get(*room).unwrap_or(&0);
        assert_eq!(*count, 1, "Room {} should have exactly 1 processed event, got {}", room, count);
    }
    assert_eq!(processed_by_room.len(), 3, "Should have processed events from 3 rooms");
}

// =============================================================================
// SCENARIO: Concurrent senders simulate Matrix SDK delivering events
// =============================================================================
#[tokio::test]
async fn scenario_concurrent_event_delivery() {
    let (tx, mut rx) = mpsc::channel::<SimulatedEvent>(256);
    let mut dedup = EventDeduplicator::new(10000);
    let processed = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Spawn multiple tasks that send the same event (simulating concurrent delivery)
    let event_id = "$concurrent123:matrix.org";
    let mut handles = Vec::new();

    for i in 0..5 {
        let tx = tx.clone();
        let event = SimulatedEvent::new(event_id, "!room:matrix.org", &format!("From task {}", i));
        handles.push(tokio::spawn(async move {
            tx.send(event).await.unwrap();
        }));
    }

    // Wait for all senders to complete
    for handle in handles {
        handle.await.unwrap();
    }
    drop(tx);

    // Process with deduplication
    let processed_clone = processed.clone();
    while let Some(event) = rx.recv().await {
        if dedup.check_and_mark(&event.event_id) {
            processed_clone.lock().unwrap().push(event.event_id.clone());
        }
    }

    let final_processed = processed.lock().unwrap();
    assert_eq!(final_processed.len(), 1, "Concurrent delivery of same event should result in 1 processed");
}

// =============================================================================
// SCENARIO: Cache overflow behavior
// =============================================================================
#[tokio::test]
async fn scenario_cache_overflow_allows_reprocess() {
    let mut dedup = EventDeduplicator::new(5); // Small cache for testing

    // Fill the cache
    for i in 0..5 {
        assert!(dedup.check_and_mark(&format!("$event{}:matrix.org", i)));
    }

    // Event 0 is now in cache
    assert!(!dedup.check_and_mark("$event0:matrix.org"), "Should reject duplicate before overflow");

    // Add one more to trigger overflow
    assert!(dedup.check_and_mark("$event5:matrix.org"));

    // After overflow, old events can be reprocessed (acceptable behavior)
    // This is a trade-off: we accept occasional reprocessing after cache clear
    // rather than unbounded memory growth
    assert!(dedup.check_and_mark("$event0:matrix.org"), "Should allow reprocess after cache clear");
}

// =============================================================================
// SCENARIO: Realistic Matrix event IDs
// =============================================================================
#[tokio::test]
async fn scenario_realistic_matrix_event_ids() {
    let mut dedup = EventDeduplicator::new(10000);

    // Real Matrix event IDs are base64-encoded with server suffix
    let real_event_ids = vec![
        "$aGVsbG8gd29ybGQgdGhpcyBpcyBhIHRlc3Q:matrix.org",
        "$Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MA:example.com",
        "$YW5vdGhlcl9ldmVudF9pZF9oZXJl:synapse.local",
        "$c29tZV9sb25nX2V2ZW50X2lkX3dpdGhfbG90c19vZl9jaGFyYWN0ZXJz:homeserver.tld",
    ];

    for event_id in &real_event_ids {
        assert!(dedup.check_and_mark(event_id), "First occurrence should be processed");
    }

    for event_id in &real_event_ids {
        assert!(!dedup.check_and_mark(event_id), "Second occurrence should be rejected");
    }
}

// =============================================================================
// SCENARIO: High throughput - many unique events
// =============================================================================
#[tokio::test]
async fn scenario_high_throughput_unique_events() {
    let mut dedup = EventDeduplicator::new(10000);
    let event_count = 1000;
    let mut processed = 0;

    for i in 0..event_count {
        if dedup.check_and_mark(&format!("$event{}:matrix.org", i)) {
            processed += 1;
        }
    }

    assert_eq!(processed, event_count, "All {} unique events should be processed", event_count);
}

// =============================================================================
// SCENARIO: Interleaved duplicates and unique events
// =============================================================================
#[tokio::test]
async fn scenario_interleaved_duplicates_and_unique() {
    let mut dedup = EventDeduplicator::new(10000);
    let mut processed = Vec::new();
    let mut skipped = 0;

    // Pattern: unique, dup, unique, dup, dup, unique
    let events = vec![
        ("$a:m.org", true),  // unique - should process
        ("$a:m.org", false), // dup - should skip
        ("$b:m.org", true),  // unique - should process
        ("$b:m.org", false), // dup - should skip
        ("$a:m.org", false), // dup - should skip (already seen earlier)
        ("$c:m.org", true),  // unique - should process
    ];

    for (event_id, should_process) in events {
        let result = dedup.check_and_mark(event_id);
        if result {
            processed.push(event_id.to_string());
        } else {
            skipped += 1;
        }
        assert_eq!(result, should_process, "Event {} processing mismatch", event_id);
    }

    assert_eq!(processed.len(), 3, "Should process 3 unique events");
    assert_eq!(skipped, 3, "Should skip 3 duplicates");
}
