use heddle::config::features::{get_features, mode_defaults, FeatureFlagsOverride, Mode};

mod common;

#[test]
fn interactive_all_true() {
    let f = mode_defaults(Mode::Interactive);
    assert!(f.history);
    assert!(f.usage_data);
    assert!(f.facets);
    assert!(f.file_history);
    assert!(f.paste_cache);
    assert!(f.status_line);
    assert!(f.hooks);
    assert!(f.tasks);
}

#[test]
fn non_interactive_disables_history_and_status() {
    let f = mode_defaults(Mode::NonInteractive);
    assert!(!f.history);
    assert!(!f.status_line);
    assert!(f.usage_data);
    assert!(f.facets);
    assert!(f.file_history);
    assert!(f.paste_cache);
    assert!(f.hooks);
    assert!(f.tasks);
}

#[test]
fn headless_disables_several_features() {
    let f = mode_defaults(Mode::Headless);
    assert!(!f.history);
    assert!(!f.facets);
    assert!(!f.status_line);
    assert!(!f.paste_cache);
    assert!(f.usage_data);
    assert!(f.file_history);
    assert!(f.hooks);
    assert!(f.tasks);
}

#[test]
fn get_features_defaults_when_no_overrides() {
    let f = get_features(Mode::Interactive, None);
    assert_eq!(f, mode_defaults(Mode::Interactive));
}

#[test]
fn get_features_merges_overrides() {
    let f = get_features(
        Mode::Interactive,
        Some(&FeatureFlagsOverride {
            history: Some(false),
            ..Default::default()
        }),
    );
    assert!(!f.history);
    assert!(f.usage_data);
}

#[test]
fn overrides_can_enable_disabled_flags() {
    let f = get_features(
        Mode::Headless,
        Some(&FeatureFlagsOverride {
            history: Some(true),
            facets: Some(true),
            ..Default::default()
        }),
    );
    assert!(f.history);
    assert!(f.facets);
    assert!(!f.status_line);
}

#[test]
fn ignores_none_override_values() {
    let f = get_features(
        Mode::Interactive,
        Some(&FeatureFlagsOverride {
            history: None,
            ..Default::default()
        }),
    );
    assert!(f.history);
}

#[test]
fn all_modes_produce_valid_flags() {
    for mode in [Mode::Interactive, Mode::NonInteractive, Mode::Headless] {
        let _f = get_features(mode, None);
    }
}
