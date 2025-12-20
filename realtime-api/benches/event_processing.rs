use criterion::{black_box, criterion_group, criterion_main, Criterion};
use realtime_api::models::Event;
use serde_json::json;

fn benchmark_event_serialization(c: &mut Criterion) {
    let event = Event {
        id: "test-event-id".to_string(),
        tenant_id: "test-tenant".to_string(),
        project_id: "test-project".to_string(),
        topic: "test-topic".to_string(),
        payload: json!({"test": "data", "number": 42}),
        published_at: chrono::Utc::now(),
    };

    c.bench_function("event_serialization", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(black_box(&event)).unwrap();
            black_box(serialized);
        })
    });
}

fn benchmark_event_deserialization(c: &mut Criterion) {
    let event = Event {
        id: "test-event-id".to_string(),
        tenant_id: "test-tenant".to_string(),
        project_id: "test-project".to_string(),
        topic: "test-topic".to_string(),
        payload: json!({"test": "data", "number": 42}),
        published_at: chrono::Utc::now(),
    };
    
    let serialized = serde_json::to_string(&event).unwrap();

    c.bench_function("event_deserialization", |b| {
        b.iter(|| {
            let deserialized: Event = serde_json::from_str(black_box(&serialized)).unwrap();
            black_box(deserialized);
        })
    });
}

criterion_group!(benches, benchmark_event_serialization, benchmark_event_deserialization);
criterion_main!(benches);