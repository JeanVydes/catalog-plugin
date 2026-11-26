use super::*;

#[test]
fn a_primary_weekly_window_keeps_its_actual_label() {
    let usage = serde_json::from_str::<Usage>(
        r#"{"rateLimits":{"primary":{"usedPercent":31,"windowDurationMins":10080}}}"#,
    )
    .unwrap();
    let mut state = State::default();
    state.finish(Some(usage));
    assert_eq!(
        state.view(Instant::now()).content,
        [Widget::Progress {
            label: "Week left".to_owned(),
            percent: 69,
            resets_at: None
        }]
    );
}

#[test]
fn both_windows_keep_their_own_percentages_and_reset_times() {
    let usage = serde_json::from_str::<Usage>(
        r#"{"rateLimits":{"primary":{"usedPercent":28,"windowDurationMins":300,"resetsAt":123},
        "secondary":{"usedPercent":65,"windowDurationMins":10080,"resetsAt":456}}}"#,
    )
    .unwrap();
    let mut state = State::default();
    state.finish(Some(usage));
    assert_eq!(
        state.view(Instant::now()).content,
        [
            Widget::Progress {
                label: "5h left".to_owned(),
                percent: 72,
                resets_at: Some(123)
            },
            Widget::Progress {
                label: "Week left".to_owned(),
                percent: 35,
                resets_at: Some(456)
            },
        ]
    );
}
