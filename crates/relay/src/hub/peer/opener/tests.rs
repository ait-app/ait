use super::*;

#[test]
fn a_call_given_up_at_its_limit_says_how_far_it_got() {
    let at = |stage: u8| {
        let progress = Progress::queued();
        assert!(progress.reach(stage));
        progress
    };
    assert_eq!(Progress::queued().give_up(), Error::Capacity);
    assert_eq!(at(OPENING).give_up(), Error::Offline);
    assert_eq!(at(HELD).give_up(), Error::Capacity);
    assert_eq!(at(SENT).give_up(), Error::OutcomeUnknown);
}

#[test]
fn a_call_given_up_is_never_sent() {
    let progress = Progress::queued();
    assert!(progress.reach(OPENING));
    assert_eq!(progress.give_up(), Error::Offline);
    assert!(!progress.reach(HELD));
    assert!(!progress.reach(SENT));
}
