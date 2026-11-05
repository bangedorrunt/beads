use beads::util::id::{IdConfig, IdGenerator};
use chrono::Utc;

#[test]
fn test_id_generator_fallback_collision() {
    let config = IdConfig {
        prefix: "bd".to_string(),
        min_hash_length: 3,
        max_hash_length: 3,
        max_collision_prob: 0.0,
    };
    let generator = IdGenerator::new(config);

    let mut existing_ids = std::collections::HashSet::new();

    let title = "Test Issue";
    let now = Utc::now();

    for nonce in 0..10 {
        let candidate = generator.generate_candidate(title, None, None, now, nonce, 3);
        existing_ids.insert(candidate);
    }

    let fallback_candidate = generator.generate_candidate(title, None, None, now, 0, 12);
    existing_ids.insert(fallback_candidate);

    let generated = generator
        .generate(
            title,
            None,
            None,
            now,
            0,
            |id| Ok(existing_ids.contains(id)),
        )
        .expect("a free fallback candidate exists");

    // If it returns the blocked fallback ID, it failed the safety contract.
    assert!(
        !existing_ids.contains(&generated),
        "Generator returned an existing ID: {generated}"
    );
}
