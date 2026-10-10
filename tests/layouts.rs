use opterm::{
    layout::Axis,
    layouts::{Node, SavedTab, SessionRef, Store, Window},
};
fn pane(id: u64) -> Node {
    Node::Pane {
        session: SessionRef {
            instance: "service-test".into(),
            id,
        },
        title: format!("Shell {id}"),
    }
}
fn arrange(window: &mut Window) {
    window.size = [880.0, 620.0];
    window.position = Some([80.0, 90.0]);
    window.maximized = true;
    window.tabs = vec![
        SavedTab {
            name: Some("Agents".into()),
            focused: 2,
            maximized: true,
            layout: Node::Split {
                axis: Axis::Right,
                ratio: 0.37,
                first: Box::new(pane(1)),
                second: Box::new(Node::Split {
                    axis: Axis::Below,
                    ratio: 0.61,
                    first: Box::new(pane(2)),
                    second: Box::new(pane(3)),
                }),
            },
        },
        SavedTab {
            name: None,
            focused: 0,
            maximized: false,
            layout: pane(4),
        },
    ];
    window.active = 1;
}
#[test]
fn layouts_round_trip_and_claim_each_window_without_overwriting_others() {
    let temp = tempfile::tempdir().unwrap();
    let mut first = Store::open(temp.path(), None, true).unwrap().unwrap();
    assert!(!first.restored);
    arrange(&mut first.store.window);
    let expected = first.store.window.clone();
    first.store.save(expected.clone()).unwrap();
    let mut second = Store::open(temp.path(), None, false).unwrap().unwrap();
    let second_id = second.store.window.id.clone();
    let mut second_window = second.store.window.clone();
    second_window.tabs = vec![SavedTab {
        name: Some("Build".into()),
        focused: 0,
        maximized: false,
        layout: pane(5),
    }];
    second.store.save(second_window.clone()).unwrap();
    assert!(
        Store::open(temp.path(), Some(&expected.id), true)
            .unwrap()
            .is_none()
    );
    drop(first);
    drop(second);
    let restored = Store::open(temp.path(), Some(&expected.id), true)
        .unwrap()
        .unwrap();
    assert!(restored.restored);
    assert_eq!(restored.store.window, expected);
    let other = Store::open(temp.path(), None, true).unwrap().unwrap();
    assert!(other.restored);
    assert_eq!(other.store.window, second_window);
    assert!(other.others.is_empty());
    assert_eq!(other.store.window.id, second_id);
    drop(restored);
    drop(other);
    let all = Store::open(temp.path(), None, true).unwrap().unwrap();
    assert_eq!(all.others.len(), 1);
    let child = Store::open(temp.path(), Some(&all.others[0]), true)
        .unwrap()
        .unwrap();
    assert_ne!(child.store.window.id, all.store.window.id);
}
#[test]
fn corrupt_and_invalid_layouts_preserve_the_saved_file() {
    let temp = tempfile::tempdir().unwrap();
    let mut boot = Store::open(temp.path(), None, true).unwrap().unwrap();
    arrange(&mut boot.store.window);
    let valid = boot.store.window.clone();
    boot.store.save(valid.clone()).unwrap();
    let path = temp
        .path()
        .join("layouts")
        .join(format!("{}.json", valid.id));
    let before = std::fs::read(&path).unwrap();
    let mut invalid = valid.clone();
    invalid.active = 99;
    assert!(boot.store.save(invalid).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut invalid = valid.clone();
    invalid.tabs[0].focused = 100;
    assert!(boot.store.save(invalid).is_err());
    let mut invalid = valid.clone();
    invalid.tabs.push(invalid.tabs[1].clone());
    assert!(boot.store.save(invalid).is_err());
    let mut invalid = valid.clone();
    invalid.size[0] = f32::NAN;
    assert!(boot.store.save(invalid).is_err());
    drop(boot);
    std::fs::write(&path, b"broken JSON").unwrap();
    let boot = Store::open(temp.path(), None, true).unwrap().unwrap();
    assert!(!boot.restored);
    assert!(boot.notice.unwrap().contains("left untouched"));
    assert_eq!(std::fs::read(&path).unwrap(), b"broken JSON");
    assert!(Store::open(temp.path(), Some("../../anything"), true).is_err());
}
#[test]
fn references_require_the_same_service_instance_and_validation_is_bounded() {
    let reference = SessionRef {
        instance: "first-service".into(),
        id: 1,
    };
    assert!(reference.matches("first-service", &[1]));
    assert!(!reference.matches("second-service", &[1]));
    assert!(!reference.matches("", &[1]));
    assert!(!reference.matches("first-service", &[2]));
    assert!(Window::decode(&vec![b' '; 262145]).is_err());
    let temp = tempfile::tempdir().unwrap();
    let boot = Store::open(temp.path(), None, false).unwrap().unwrap();
    let mut window = boot.store.window.clone();
    window.tabs = vec![SavedTab {
        name: None,
        focused: 0,
        maximized: false,
        layout: Node::Split {
            axis: Axis::Right,
            ratio: 1.5,
            first: Box::new(pane(1)),
            second: Box::new(pane(2)),
        },
    }];
    assert!(window.validate().is_err());
}
#[cfg(unix)]
#[test]
fn files_are_private_and_write_failures_leave_the_previous_layout() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let mut boot = Store::open(temp.path(), None, false).unwrap().unwrap();
    let window = boot.store.window.clone();
    boot.store.save(window.clone()).unwrap();
    let directory = temp.path().join("layouts");
    let path = directory.join(format!("{}.json", window.id));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let before = std::fs::read(&path).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut changed = window;
    changed.size[0] = 1001.0;
    assert!(boot.store.save(changed).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
}
