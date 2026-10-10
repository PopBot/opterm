use opterm::{
    profiles::{Environment, Profile, Rgb, Theme},
    settings::{Paths, Settings, Store},
};
fn store(temp: &tempfile::TempDir) -> Store {
    Store::load(Paths {
        config: temp.path().join("settings/settings.toml"),
        backup: temp.path().join("recovery/last-valid-settings.toml"),
    })
}
fn configured() -> Settings {
    let theme = Theme {
        id: "custom".into(),
        name: "Custom".into(),
        ..Theme::light()
    };
    let profile = Profile {
        id: "agent".into(),
        name: "Agent".into(),
        executable: "codex".into(),
        arguments: vec!["a single argument with spaces".into()],
        theme: theme.id.clone(),
        environment: vec![
            Environment {
                name: "VISIBLE".into(),
                value: "ordinary value".into(),
                secret: false,
            },
            Environment {
                name: "PRIVATE".into(),
                value: "fake-secret-for-test".into(),
                secret: true,
            },
        ],
        ..Profile::default()
    };
    Settings {
        profiles: vec![profile],
        themes: vec![theme],
        default_profile: "agent".into(),
        ..Settings::default()
    }
}
#[test]
fn profiles_themes_and_recovery_omit_only_marked_secret_values() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(&temp);
    let draft = configured();
    store.save(&draft, &store.observed.clone(), false).unwrap();
    assert!(
        std::fs::read_to_string(&store.paths.config)
            .unwrap()
            .contains("fake-secret-for-test")
    );
    let backup = std::fs::read(&store.paths.backup).unwrap();
    assert!(!String::from_utf8_lossy(&backup).contains("fake-secret-for-test"));
    let recovered = store.recovery().unwrap();
    assert_eq!(recovered.themes, draft.themes);
    assert_eq!(recovered.profiles[0].arguments, draft.profiles[0].arguments);
    assert_eq!(recovered.profiles[0].environment[0].value, "ordinary value");
    assert!(recovered.profiles[0].environment[1].secret);
    assert!(recovered.profiles[0].environment[1].value.is_empty());
    assert!(
        recovered
            .launch("agent")
            .unwrap()
            .directory()
            .unwrap_err()
            .to_string()
            .contains("Reenter")
    );
    std::fs::write(&store.paths.config, "invalid = [").unwrap();
    let loaded = Store::load(store.paths.clone());
    assert_eq!(loaded.current, recovered);
    assert_eq!(std::fs::read(&store.paths.backup).unwrap(), backup);
    // Repairing invalid edits must never advance the good backup to malformed profiles.
    let mut invalid = draft.clone();
    invalid.themes[0].id = "dark".into();
    assert!(store.save(&invalid, &store.disk_revision(), true).is_err());
    assert_eq!(std::fs::read(&store.paths.backup).unwrap(), backup);
}
#[test]
fn appearance_only_v1_migrates_and_deleted_references_have_safe_fallbacks() {
    let old = b"version = 1\n[appearance]\nfont_size = 16.0\n";
    let settings = Settings::parse(old).unwrap();
    assert_eq!(settings.version, 2);
    assert_eq!(settings.appearance.font_size, 16.0);
    assert_eq!(settings.default_profile().id, "shell");
    let mut settings = configured();
    settings.themes.clear();
    assert_eq!(settings.launch("agent").unwrap().theme, Theme::dark());
    assert!(settings.reference_notice().unwrap().contains("removed"));
    settings.profiles.clear();
    assert_eq!(settings.default_profile().id, "shell");
    assert!(settings.launch("agent").is_err());
}
#[test]
fn invalid_env_arguments_and_oversized_files_cannot_replace_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(&temp);
    let original = Settings::default();
    store
        .save(&original, &store.observed.clone(), false)
        .unwrap();
    let good = std::fs::read(&store.paths.backup).unwrap();
    let mut invalid = configured();
    invalid.profiles[0].environment.push(Environment {
        name: "visible".into(),
        value: "duplicate".into(),
        secret: false,
    });
    assert!(
        store
            .save(&invalid, &store.observed.clone(), false)
            .is_err()
    );
    invalid = configured();
    invalid.profiles[0].arguments.push("bad\0argument".into());
    assert!(invalid.encode().is_err());
    invalid = configured();
    invalid.profiles[0].environment = (0..12)
        .map(|i| Environment {
            name: format!("VAR_{i}"),
            value: "x".repeat(8192),
            secret: false,
        })
        .collect();
    assert!(
        store
            .save(&invalid, &store.observed.clone(), false)
            .is_err()
    );
    assert_eq!(std::fs::read(&store.paths.backup).unwrap(), good);
}
#[test]
fn theme_import_export_validates_colors_and_preserves_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("theme.toml");
    let theme = configured().themes.remove(0);
    theme.export(&path).unwrap();
    assert_eq!(Theme::import(&path).unwrap(), theme);
    assert!(Theme::dark().export(&path).is_err());
    assert_eq!(Theme::import(&path).unwrap(), theme);
    for color in ["#12345", "#xyzxyz", "#é0000", "123456", "#12345678"] {
        assert!(Rgb::try_from(color.to_owned()).is_err());
    }
    std::fs::write(
        &path,
        std::fs::read_to_string(&path)
            .unwrap()
            .replace("#f8f8f9", "#nothex"),
    )
    .unwrap();
    assert!(Theme::import(&path).is_err());
}
