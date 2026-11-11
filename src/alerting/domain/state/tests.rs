use super::super::rule::{AlertRule, Comparison, NoData, RuleSource};
use super::*;

fn rule(hold_seconds: u64, no_data: NoData) -> AlertRule {
    AlertRule {
        version: 1,
        project: "project-a".into(),
        environment: None,
        source: RuleSource::Metrics {
            expression: "up".into(),
            comparison: Comparison::Greater,
            threshold: 0.0,
        },
        hold_seconds,
        no_data,
        webhook: "hook".into(),
        enabled: true,
        mute_until: None,
    }
}

fn state() -> AlertState {
    AlertState::new(
        "rule-1",
        BTreeMap::from([("service".to_string(), "checkout".to_string())]),
    )
}

#[test]
fn hold_time_keeps_pending_until_the_hold_has_passed() {
    let r = rule(60, NoData::Unknown);
    let mut s = state();

    assert!(s
        .observe(&r, Observation::Condition(true), 0)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Pending);

    assert!(s
        .observe(&r, Observation::Condition(true), 59)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Pending);

    let n = s
        .observe(&r, Observation::Condition(true), 60)
        .unwrap()
        .expect("firing notification");
    assert_eq!(n.transition, "firing");
    assert_eq!(s.phase, Phase::Firing);
    assert_eq!(s.episode, 1);
    assert!(s.active_episode);
    assert_eq!(n.created_at, 60);
    assert_eq!(n.webhook, "hook");
    assert_eq!(n.project, "project-a");
}

#[test]
fn a_second_observation_of_the_same_firing_makes_no_second_notification() {
    let r = rule(60, NoData::Unknown);
    let mut first = state();
    first.observe(&r, Observation::Condition(true), 0).unwrap();
    let fired = first
        .observe(&r, Observation::Condition(true), 60)
        .unwrap()
        .expect("firing notification");

    assert!(first
        .observe(&r, Observation::Condition(true), 61)
        .unwrap()
        .is_none());

    let mut second = state();
    second.observe(&r, Observation::Condition(true), 0).unwrap();
    let again = second
        .observe(&r, Observation::Condition(true), 60)
        .unwrap()
        .expect("firing notification");
    assert!(!fired.id.is_empty());
    assert_eq!(again.id, fired.id);
}

#[test]
fn a_notified_episode_resolves_once() {
    let r = rule(60, NoData::Unknown);
    let mut s = state();
    s.observe(&r, Observation::Condition(true), 0).unwrap();
    let fired = s
        .observe(&r, Observation::Condition(true), 60)
        .unwrap()
        .expect("firing notification");

    let resolved = s
        .observe(&r, Observation::Condition(false), 70)
        .unwrap()
        .expect("resolved notification");
    assert_eq!(resolved.transition, "resolved");
    assert_eq!(resolved.payload["episode"], 1);
    assert_eq!(s.phase, Phase::Inactive);
    assert!(!s.active_episode);
    assert_ne!(resolved.id, fired.id);

    assert!(s
        .observe(&r, Observation::Condition(false), 80)
        .unwrap()
        .is_none());
}

#[test]
fn unknown_never_resolves_an_active_episode() {
    let r = rule(60, NoData::Unknown);
    let mut s = state();
    s.observe(&r, Observation::Condition(true), 0).unwrap();
    s.observe(&r, Observation::Condition(true), 60).unwrap();

    assert!(s
        .observe(&r, Observation::Unknown("db down".into()), 70)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Unknown);
    assert_eq!(s.reason, Some("db down".to_string()));
    assert!(s.active_episode);

    assert!(s
        .observe(&r, Observation::Condition(true), 80)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Firing);

    let resolved = s
        .observe(&r, Observation::Condition(false), 90)
        .unwrap()
        .expect("resolved notification");
    assert_eq!(resolved.transition, "resolved");
}

#[test]
fn no_data_follows_the_rule_setting() {
    // NoData::Unknown: no notification, phase Unknown with the reason.
    let unknown_rule = rule(0, NoData::Unknown);
    let mut s = state();
    assert!(s
        .observe(&unknown_rule, Observation::NoData, 0)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Unknown);
    assert_eq!(s.reason, Some("no data".to_string()));

    // NoData::Firing: a fresh state fires at once.
    let firing_rule = rule(0, NoData::Firing);
    let mut s = state();
    let fired = s
        .observe(&firing_rule, Observation::NoData, 0)
        .unwrap()
        .expect("firing notification");
    assert_eq!(fired.transition, "firing");
    assert_eq!(s.phase, Phase::Firing);

    // NoData::Inactive: an active episode resolves; a fresh state stays inactive.
    let inactive_rule = rule(0, NoData::Inactive);
    let mut s = state();
    s.observe(&inactive_rule, Observation::Condition(true), 0)
        .unwrap();
    let resolved = s
        .observe(&inactive_rule, Observation::NoData, 10)
        .unwrap()
        .expect("resolved notification");
    assert_eq!(resolved.transition, "resolved");
    assert_eq!(s.phase, Phase::Inactive);

    let mut s = state();
    assert!(s
        .observe(&inactive_rule, Observation::NoData, 0)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Inactive);
}

#[test]
fn a_future_mute_makes_no_notification() {
    // Rule mute.
    let mut muted = rule(0, NoData::Unknown);
    muted.mute_until = Some(100);
    let mut s = state();
    assert!(s
        .observe(&muted, Observation::Condition(true), 50)
        .unwrap()
        .is_none());
    assert_eq!(s.phase, Phase::Firing);
    let fired = s
        .observe(&muted, Observation::Condition(true), 100)
        .unwrap()
        .expect("firing notification");
    assert_eq!(fired.transition, "firing");

    // State mute.
    let plain = rule(0, NoData::Unknown);
    let mut s = state();
    s.mute_until = Some(100);
    assert!(s
        .observe(&plain, Observation::Condition(true), 50)
        .unwrap()
        .is_none());
    let fired = s
        .observe(&plain, Observation::Condition(true), 100)
        .unwrap()
        .expect("firing notification");
    assert_eq!(fired.transition, "firing");
}

#[test]
fn event_fires_once_per_new_cursor() {
    let r = rule(0, NoData::Unknown);
    let mut s = state();

    let first = s
        .observe(&r, Observation::Event(5), 0)
        .unwrap()
        .expect("firing notification");
    assert_eq!(first.transition, "firing");
    assert_eq!(s.last_event_cursor, 5);
    assert_eq!(s.episode, 1);

    assert!(s.observe(&r, Observation::Event(5), 1).unwrap().is_none());

    let second = s
        .observe(&r, Observation::Event(6), 2)
        .unwrap()
        .expect("firing notification");
    assert_eq!(second.transition, "firing");
    assert_eq!(s.episode, 2);
    assert_ne!(second.id, first.id);

    assert!(s.observe(&r, Observation::Event(6), 3).unwrap().is_none());

    assert!(s.observe(&r, Observation::Event(3), 4).unwrap().is_none());
    assert_eq!(s.last_event_cursor, 6);
}

#[test]
fn time_moving_backwards_is_an_error() {
    let r = rule(0, NoData::Unknown);
    let mut s = state();
    assert!(s
        .observe(&r, Observation::Condition(false), 100)
        .unwrap()
        .is_none());

    let err = s
        .observe(&r, Observation::Condition(false), 99)
        .unwrap_err();
    assert_eq!(err, "alert clock moved backwards".to_string());
    assert_eq!(s.last_evaluated, 100);
}

#[test]
fn the_notification_id_is_the_same_for_the_same_input() {
    let r = rule(0, NoData::Unknown);
    let mut a = state();
    let mut b = state();
    let na = a
        .observe(&r, Observation::Condition(true), 5)
        .unwrap()
        .expect("firing notification");
    let nb = b
        .observe(&r, Observation::Condition(true), 5)
        .unwrap()
        .expect("firing notification");
    assert_eq!(na.id, nb.id);

    let mut other = AlertState::new("rule-1", BTreeMap::new());
    let no = other
        .observe(&r, Observation::Condition(true), 5)
        .unwrap()
        .expect("firing notification");
    assert_ne!(no.id, na.id);
}
