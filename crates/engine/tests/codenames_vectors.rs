//! `VectorTable` tests: the text-format parser and the cosine it scores with.
//!
//! Split out of `codenames_embedding.rs`, which exercises the bot that *uses*
//! the table; this file is about the table itself. The fixture geometry is
//! exact by construction (see the fixture's own header), so every similarity
//! below has one right answer rather than a plausible range.
//!
//! Both halves are strictness tests at heart: a truncated or ragged vector file
//! must fail loudly at load time, and `cosine` must never hand the anchor a
//! `NaN` to rank against.

use engine::codenames::agents::{cosine, VectorTable, VectorTableError};
use std::sync::Arc;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/codenames_test_vectors.txt"
);

fn table() -> Arc<VectorTable> {
    Arc::new(VectorTable::from_path(FIXTURE).expect("fixture vector table parses"))
}

#[test]
fn the_fixture_table_parses_and_malformed_lines_are_rejected() {
    let table = table();
    assert_eq!(table.dim(), 12);
    assert_eq!(table.len(), 76, "8 clusters x 9 words + 4 distractors");
    assert!(table.contains("music") && table.contains("guitar"));
    assert!(!table.contains("nonesuch"));
    // Lookups are case-insensitive, and words come back lexicographically.
    assert_eq!(table.get("GUITAR"), table.get("guitar"));
    let mut sorted: Vec<&str> = table.words().collect();
    assert_eq!(sorted.first(), Some(&"airplane"));
    sorted.sort_unstable();
    assert_eq!(sorted, table.words().collect::<Vec<_>>());

    let text = std::fs::read_to_string(FIXTURE).expect("fixture readable");
    assert_eq!(VectorTable::parse(&text).as_ref(), Ok(&*table));

    let good = "# comment\nalpha 1.0 0.0\n\nbravo 0.0 1.0\n";
    assert_eq!(VectorTable::parse(good).expect("valid").len(), 2);
    // A word2vec `<count> <dim>` header is tolerated; a 1-token line is not.
    assert_eq!(
        VectorTable::parse(&format!("2 2\n{good}"))
            .expect("header skipped")
            .len(),
        2
    );
    // …but the header is held to its own promise. A count or dimension that
    // disagrees with the body is exactly how a truncated download presents
    // itself, and it must not load as a silently half-populated table.
    assert!(matches!(
        VectorTable::parse(&format!("3 2\n{good}")),
        Err(VectorTableError::HeaderMismatch {
            declared_count: 3,
            found_count: 2,
            ..
        })
    ));
    assert!(matches!(
        VectorTable::parse(&format!("2 3\n{good}")),
        Err(VectorTableError::HeaderMismatch {
            declared_dim: 3,
            found_dim: 2,
            ..
        })
    ));
    // A header whose body was cut short mid-file is the real failure mode.
    assert!(matches!(
        VectorTable::parse("500 2\nalpha 1.0 0.0\n"),
        Err(VectorTableError::HeaderMismatch {
            declared_count: 500,
            found_count: 1,
            ..
        })
    ));
    assert!(matches!(
        VectorTable::parse("alpha 1.0 0.0\nbravo\n"),
        Err(VectorTableError::Malformed { line: 2, .. })
    ));
    assert!(matches!(
        VectorTable::parse("alpha 1.0 0.0\nbravo 1.0 0.0 3.0\n"),
        Err(VectorTableError::DimMismatch {
            line: 2,
            found: 3,
            expected: 2,
            ..
        })
    ));
    assert!(matches!(
        VectorTable::parse("alpha 1.0 x\n"),
        Err(VectorTableError::BadComponent {
            line: 1,
            index: 1,
            ..
        })
    ));
    assert!(matches!(
        VectorTable::parse("alpha 1.0 0.0\nALPHA 0.0 1.0\n"),
        Err(VectorTableError::Duplicate { line: 2, .. })
    ));
    assert!(matches!(
        VectorTable::parse("alpha 1.0 nan\n"),
        Err(VectorTableError::BadComponent { .. })
    ));
    assert_eq!(
        VectorTable::parse("# nothing\n"),
        Err(VectorTableError::Empty)
    );
    assert!(matches!(
        VectorTable::from_path("/nonexistent/vectors.txt"),
        Err(VectorTableError::Io { .. })
    ));
}

#[test]
fn cosine_reproduces_the_fixture_geometry_exactly() {
    let table = table();
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let sim = |a: &str, b: &str| table.similarity(a, b).expect("both words are tabled");

    // A centroid sits at 0.98 from its own cluster and 0.00 from every other.
    for word in [
        "bear", "cat", "dog", "horse", "lion", "monkey", "tiger", "wolf",
    ] {
        assert!(close(sim("animal", word), 0.98), "animal vs {word}");
        assert!(close(sim("music", word), 0.0), "music vs {word}");
    }
    // Cluster members are closer to their centroid than to each other, which
    // is what makes the centroid the strictly best clue for the cluster.
    assert!(close(sim("dog", "cat"), 0.9604));
    assert!(close(sim("dog", "tiger"), 0.9208), "opposed residuals");
    assert!(sim("animal", "dog") > sim("dog", "cat"));
    // Cross-cluster leakage is bounded by the shared residual axes.
    assert!(sim("dog", "ocean").abs() < 0.05);

    assert_eq!(cosine(&[1.0, 0.0], &[2.0, 0.0]), 1.0, "scale invariant");
    assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    assert_eq!(cosine(&[1.0, 0.0], &[-1.0, 0.0]), -1.0);
    assert_eq!(
        cosine(&[0.0, 0.0], &[1.0, 1.0]),
        0.0,
        "zero norm is not NaN"
    );
    assert_eq!(
        cosine(&[1.0], &[1.0, 0.0]),
        0.0,
        "ragged pair is not a panic"
    );
    // Large but finite components: squaring these overflows an f32 accumulator
    // to infinity, and inf/inf is the NaN that would poison the anchor's max.
    let big = [1e20f32, 0.0];
    assert_eq!(cosine(&big, &big), 1.0, "f32 accumulators would give NaN");
    assert_eq!(cosine(&big, &[0.0, 1e20]), 0.0);
    assert_eq!(cosine(&big, &[-1e20, 0.0]), -1.0);
    for c in [f32::MAX, f32::MIN_POSITIVE] {
        let v = [c, c];
        assert!(
            cosine(&v, &v).is_finite() && !cosine(&v, &v).is_nan(),
            "cosine is never NaN for finite input ({c:e})"
        );
    }
    assert_eq!(table.similarity("dog", "nonesuch"), None);
}
