use super::*;

#[test]
fn ignores_loopback_and_duplicate_virtual_traffic() {
    for name in [
        "lo", "lo0", "docker0", "veth123", "br-a", "virbr0", "bridge0", "utun2", "awdl0", "llw0",
    ] {
        assert!(!include_interface(name), "{name}");
    }
    for name in ["eth0", "en0", "wlan0", "enp5s0"] {
        assert!(include_interface(name));
    }
}

#[test]
fn computes_rates_from_elapsed_time_without_overflow() {
    assert_eq!(bytes_per_second(4096, Duration::from_secs(2)), 2048);
    assert_eq!(bytes_per_second(9, Duration::from_millis(1500)), 6);
    assert_eq!(bytes_per_second(u64::MAX, Duration::from_secs(1)), u64::MAX);
    assert_eq!(bytes_per_second(u64::MAX, Duration::ZERO), u64::MAX);
}

#[test]
fn sampler_construction_does_not_read_the_host() {
    assert!(LocalHostSampler::default().counters.is_none());
}
