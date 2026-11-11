use super::*;

#[test]
fn the_first_semantic_version_is_compared_with_aits_floor() {
    for (output, expected) in [
        ("omp/18.18.8", Some([18, 18, 8])),
        ("v18.12.6-beta.1", Some([18, 12, 6])),
        ("omp 1.2 then 3.4.5.6", Some([3, 4, 5])),
        ("omp/dev", None),
        ("18.12", None),
    ] {
        assert_eq!(semantic_version(output), expected, "{output}");
    }
    assert_eq!(
        support("omp/18.18.8"),
        "18.18.8 (supported; minimum 18.12.6)"
    );
    assert_eq!(
        support("omp/18.12.6"),
        "18.12.6 (supported; minimum 18.12.6)"
    );
    assert_eq!(
        support("omp/18.12.5"),
        "18.12.5 (unsupported; minimum 18.12.6)"
    );
    assert_eq!(
        support("omp/16.3.9"),
        "16.3.9 (unsupported; minimum 18.12.6)"
    );
    assert_eq!(support("omp/dev"), "unknown (minimum 18.12.6)");
}

#[test]
fn rows_stay_on_one_line() {
    let mut report = String::from("OMP");
    row(&mut report, "Config root", "/home/a\nb\u{7}");
    assert_eq!(report, "OMP\n  Config root: /home/a?b?");
}
