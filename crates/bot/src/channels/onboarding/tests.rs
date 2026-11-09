use super::*;

fn expired_scan() -> Scan<(), ()> {
    Scan::new(reqwest::Client::new(), (), Duration::ZERO, 0, false)
}

#[tokio::test]
async fn a_scan_past_its_deadline_expires() {
    let mut scan = expired_scan();
    assert_eq!(scan.before_poll(), Some(failed(FAILED_EXPIRED)));
}

#[tokio::test]
async fn approved_credentials_outlive_the_deadline_so_a_save_can_retry() {
    let mut scan = expired_scan();
    scan.approved = Some(());
    assert_eq!(scan.before_poll(), None);
}
