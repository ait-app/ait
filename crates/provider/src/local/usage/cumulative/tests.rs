use super::{CumulativeUsage, UsageCounters};

const fn counters(input: u64, output: u64, cache_read: u64, cache_write: u64) -> UsageCounters {
    UsageCounters {
        input,
        output,
        cache_read,
        cache_write,
    }
}

#[test]
fn first_total_is_only_a_baseline() {
    let mut stream = CumulativeUsage::default();

    assert_eq!(stream.observe(counters(100, 20, 50, 10)), None);
    assert_eq!(
        stream.observe(counters(130, 25, 50, 12)),
        Some(counters(30, 5, 0, 2))
    );
}

#[test]
fn growth_is_differenced_against_the_latest_total() {
    let mut stream = CumulativeUsage::default();
    stream.observe(counters(10, 1, 0, 0));

    assert_eq!(
        stream.observe(counters(15, 3, 4, 0)),
        Some(counters(5, 2, 4, 0))
    );
    assert_eq!(
        stream.observe(counters(16, 3, 4, 7)),
        Some(counters(1, 0, 0, 7))
    );
}

#[test]
fn unchanged_total_yields_no_delta() {
    let mut stream = CumulativeUsage::default();
    stream.observe(counters(10, 1, 2, 3));

    assert_eq!(stream.observe(counters(10, 1, 2, 3)), None);
    assert_eq!(
        stream.observe(counters(11, 1, 2, 3)),
        Some(counters(1, 0, 0, 0))
    );
}

#[test]
fn drop_in_any_counter_resets_the_baseline_without_a_delta() {
    for restarted in [
        counters(9, 50, 50, 50),
        counters(50, 0, 50, 50),
        counters(50, 50, 9, 50),
        counters(50, 50, 50, 9),
    ] {
        let mut stream = CumulativeUsage::default();
        stream.observe(counters(10, 10, 10, 10));

        assert_eq!(stream.observe(restarted), None);
        assert_eq!(
            stream.observe(counters(
                restarted.input + 1,
                restarted.output + 2,
                restarted.cache_read + 3,
                restarted.cache_write + 4,
            )),
            Some(counters(1, 2, 3, 4))
        );
    }
}
