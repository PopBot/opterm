use opterm::{
    settings::{Paths, Store},
    storage,
};
use std::{fs, path::Path};
fn paths(root: &Path) -> Paths {
    Paths {
        config: root.join("normal/settings/settings.toml"),
        backup: root.join("normal/recovery/last-valid-settings.toml"),
        cache: root.join("normal/cache"),
        locator: Some(root.join("normal/data/settings-location.json")),
        selected_root: None,
        location_notice: None,
    }
}
fn saved(root: &Path, size: f32) -> Store {
    let mut store = Store::load(paths(root));
    let mut draft = store.current.clone();
    draft.appearance.font_size = size;
    store.save(&draft, &store.disk_revision(), false).unwrap();
    store
}
#[test]
fn relocation_is_next_launch_only_and_preserves_original_and_backup() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = saved(temp.path(), 19.0);
    let config = fs::read(&store.paths.config).unwrap();
    let backup = fs::read(&store.paths.backup).unwrap();
    let original = store.paths.config.clone();
    let root = temp.path().join("chosen-opterm");
    let destination = store.stage_location(&root).unwrap();
    assert_eq!(store.paths.config, original);
    assert_eq!(fs::read(&original).unwrap(), config);
    assert_eq!(fs::read(&store.paths.backup).unwrap(), backup);
    assert_eq!(fs::read(&destination).unwrap(), config);
    let resolved = storage::discover(paths(temp.path()));
    assert_eq!(resolved.config, destination);
    assert_eq!(resolved.cache, root.join("cache"));
    assert_eq!(resolved.backup, store.paths.backup);
    let launched = Store::load(resolved);
    assert_eq!(launched.current, store.current);
    assert_eq!(fs::read(&store.paths.backup).unwrap(), backup);
    // Later writes in the original window remain at its original path.
    let mut draft = store.current.clone();
    draft.appearance.font_size = 21.0;
    store.save(&draft, &store.disk_revision(), false).unwrap();
    assert_eq!(
        opterm::settings::Settings::parse(&fs::read(destination).unwrap())
            .unwrap()
            .appearance
            .font_size,
        19.0
    );
}
#[test]
fn existing_destination_and_failed_locator_write_preserve_active_location() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = saved(temp.path(), 17.0);
    let original = fs::read(&store.paths.config).unwrap();
    let root = temp.path().join("chosen");
    store.stage_location(&root).unwrap();
    let locator = store.paths.locator.clone().unwrap();
    let before = fs::read(&locator).unwrap();
    let blocked = temp.path().join("blocked");
    fs::create_dir_all(blocked.join("settings")).unwrap();
    let destination = blocked.join("settings/settings.toml");
    fs::write(&destination, "unrelated user file").unwrap();
    assert!(store.stage_location(&blocked).is_err());
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        "unrelated user file"
    );
    assert_eq!(fs::read(&locator).unwrap(), before);
    fs::remove_file(&locator).unwrap();
    fs::create_dir(&locator).unwrap();
    assert!(
        store
            .stage_location(&temp.path().join("copy-with-failed-locator"))
            .is_err()
    );
    assert_eq!(fs::read(&store.paths.config).unwrap(), original);
    assert!(fs::metadata(locator).unwrap().is_dir());
    assert!(
        store
            .stage_location(Path::new("relative/location"))
            .is_err()
    );
}
#[test]
fn unavailable_or_corrupt_location_uses_explicit_recovery_and_blocks_fallback_saves() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = saved(temp.path(), 18.0);
    let root = temp.path().join("chosen");
    store.stage_location(&root).unwrap();
    fs::rename(&root, temp.path().join("disconnected")).unwrap();
    let selected = storage::discover(paths(temp.path()));
    assert_eq!(selected.config, root.join("settings/settings.toml"));
    assert!(selected.location_notice.is_some());
    let mut recovered = Store::load(selected);
    assert_eq!(recovered.current.appearance.font_size, 18.0);
    assert!(recovered.notice.as_ref().unwrap().contains("unavailable"));
    assert!(storage::clear_cache(&recovered.paths).is_err());
    assert!(!root.exists());
    assert!(
        recovered
            .save(&recovered.current.clone(), &recovered.disk_revision(), true)
            .is_err()
    );
    assert!(!root.exists());
    let mut fallback = store.current.clone();
    fallback.appearance.font_size = 25.0;
    fs::write(&store.paths.config, fallback.encode().unwrap()).unwrap();
    fs::write(
        store.paths.locator.as_ref().unwrap(),
        "invalid location JSON",
    )
    .unwrap();
    let mut corrupt = Store::load(storage::discover(paths(temp.path())));
    assert_eq!(corrupt.current.appearance.font_size, 18.0);
    assert!(
        corrupt
            .notice
            .as_ref()
            .unwrap()
            .contains("Default settings were not activated")
    );
    assert!(!corrupt.poll());
    let repaired = temp.path().join("repaired");
    corrupt.stage_location(&repaired).unwrap();
    assert_eq!(
        Store::load(storage::discover(paths(temp.path())))
            .current
            .appearance
            .font_size,
        18.0
    );
}
#[test]
fn clear_cache_preserves_unrelated_files_settings_state_and_backup() {
    let temp = tempfile::tempdir().unwrap();
    let store = saved(temp.path(), 16.0);
    assert_eq!(storage::clear_cache(&store.paths).unwrap(), 0);
    let owned = store.paths.cache.join("opterm-disposable");
    fs::write(owned.join("opterm-glyphs.bin"), "recreatable").unwrap();
    fs::write(owned.join("personal-notes.txt"), "keep").unwrap();
    let sibling = store.paths.cache.join("unrelated.bin");
    fs::write(&sibling, "keep").unwrap();
    let durable = temp.path().join("runtime");
    fs::create_dir(&durable).unwrap();
    for name in ["history.log", "layouts.json", "service.lock"] {
        fs::write(durable.join(name), "keep").unwrap();
    }
    let config = fs::read(&store.paths.config).unwrap();
    let backup = fs::read(&store.paths.backup).unwrap();
    assert_eq!(storage::clear_cache(&store.paths).unwrap(), 11);
    assert!(!owned.join("opterm-glyphs.bin").exists());
    assert!(owned.join("personal-notes.txt").exists());
    assert!(sibling.exists());
    assert_eq!(fs::read(&store.paths.config).unwrap(), config);
    assert_eq!(fs::read(&store.paths.backup).unwrap(), backup);
    for name in ["history.log", "layouts.json", "service.lock"] {
        assert!(durable.join(name).exists());
    }
    fs::remove_file(owned.join(".opterm-cache")).unwrap();
    fs::write(owned.join("opterm-unrelated.bin"), "keep").unwrap();
    assert!(storage::clear_cache(&store.paths).is_err());
    assert!(owned.join("opterm-unrelated.bin").exists());
}
#[cfg(unix)]
#[test]
fn clear_cache_rejects_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let store = saved(temp.path(), 15.0);
    storage::clear_cache(&store.paths).unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, "keep").unwrap();
    let link = store.paths.cache.join("opterm-disposable/opterm-link");
    symlink(&outside, &link).unwrap();
    assert_eq!(storage::clear_cache(&store.paths).unwrap(), 0);
    assert!(link.exists());
    assert_eq!(fs::read_to_string(outside).unwrap(), "keep");
}

#[test]
fn cache_overlap_with_actual_service_state_or_recovery_is_rejected_before_changes() {
    let temp = tempfile::tempdir().unwrap();
    let store = saved(temp.path(), 14.0);
    let mut dangerous = store.paths.clone();
    dangerous.cache = opterm::service::state_dir().unwrap();
    assert!(
        storage::clear_cache(&dangerous)
            .unwrap_err()
            .to_string()
            .contains("live-session")
    );
    dangerous.cache = temp.path().join("cache-never-created");
    dangerous.backup = dangerous
        .cache
        .join("opterm-disposable/last-valid-settings.toml");
    assert!(storage::clear_cache(&dangerous).is_err());
    assert!(!dangerous.cache.exists());
}
