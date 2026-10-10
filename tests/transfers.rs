use opterm::{
    layout::Axis,
    layouts::{Node, SavedTab, SessionRef, Store},
    transfers::{self, Offer, Status},
};
fn tab() -> SavedTab {
    let pane = |id| Node::Pane {
        session: SessionRef {
            instance: "service-existing".into(),
            id,
        },
        title: format!("Shell {id}"),
    };
    SavedTab {
        name: Some("Long running agent".into()),
        focused: 1,
        maximized: true,
        layout: Node::Split {
            axis: Axis::Right,
            ratio: 0.37,
            first: Box::new(pane(1)),
            second: Box::new(pane(2)),
        },
    }
}
fn window(state: &std::path::Path) -> Store {
    let mut store = Store::open(state, None, false).unwrap().unwrap().store;
    store.save(store.window.clone()).unwrap();
    store
}
#[test]
fn existing_window_transfer_preserves_tree_and_requires_one_acknowledgment() {
    let state = tempfile::tempdir().unwrap();
    let source = window(state.path());
    let target = window(state.path());
    let other = window(state.path());
    assert_eq!(transfers::live_windows(state.path()).unwrap().len(), 3);
    let mut pending = Offer::send(
        state.path(),
        &source.window.id,
        Some(&target.window.id),
        tab(),
    )
    .unwrap();
    assert_eq!(pending.poll().unwrap(), Status::Pending);
    assert!(
        transfers::incoming(state.path(), &other.window.id)
            .unwrap()
            .is_empty()
    );
    let offer = transfers::incoming(state.path(), &target.window.id)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(offer.tab, tab());
    assert!(offer.acknowledge(state.path(), &other.window.id).is_err());
    offer.acknowledge(state.path(), &target.window.id).unwrap();
    assert!(offer.acknowledge(state.path(), &target.window.id).is_err());
    assert!(
        transfers::incoming(state.path(), &target.window.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        pending.poll().unwrap(),
        Status::Accepted {
            window: target.window.id.clone()
        }
    );
    assert!(transfers::receive(state.path(), pending.id(), &target.window.id).is_err());
}
#[test]
fn new_window_offer_is_explicit_and_cancellation_rejects_late_acceptance() {
    let state = tempfile::tempdir().unwrap();
    let source = window(state.path());
    let target = window(state.path());
    let mut pending = Offer::send(state.path(), &source.window.id, None, tab()).unwrap();
    assert!(
        transfers::incoming(state.path(), &target.window.id)
            .unwrap()
            .is_empty()
    );
    let offer = transfers::receive(state.path(), pending.id(), &target.window.id).unwrap();
    pending.cancel().unwrap();
    assert_eq!(pending.poll().unwrap(), Status::Expired);
    assert!(offer.acknowledge(state.path(), &target.window.id).is_err());
    assert_eq!(source.window.tabs.len(), 0); // Transfer storage never edits either layout.
    assert!(transfers::receive(state.path(), "../../elsewhere", &target.window.id).is_err());
    assert!(
        Offer::send(
            state.path(),
            &source.window.id,
            Some(&source.window.id),
            tab()
        )
        .is_err()
    );
    let target_id = target.window.id.clone();
    drop(target);
    assert_eq!(transfers::live_windows(state.path()).unwrap().len(), 1);
    assert!(Offer::send(state.path(), &source.window.id, Some(&target_id), tab()).is_err());
}
#[test]
fn malformed_transfer_and_expired_offer_cannot_be_accepted() {
    let state = tempfile::tempdir().unwrap();
    let source = window(state.path());
    let target = window(state.path());
    let mut pending = Offer::send(
        state.path(),
        &source.window.id,
        Some(&target.window.id),
        tab(),
    )
    .unwrap();
    let path = state
        .path()
        .join("transfers")
        .join(format!("{}.json", pending.id()));
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["expires"] = 0.into();
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        transfers::incoming(state.path(), &target.window.id)
            .unwrap()
            .is_empty()
    );
    assert!(transfers::receive(state.path(), pending.id(), &target.window.id).is_err());
    pending.cancel().unwrap();
    let mut invalid = tab();
    invalid.focused = 500;
    assert!(
        Offer::send(
            state.path(),
            &source.window.id,
            Some(&target.window.id),
            invalid
        )
        .is_err()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn only_one_new_window_can_accept_an_offer_and_files_are_private() {
    let state = tempfile::tempdir().unwrap();
    let source = window(state.path());
    let target = window(state.path());
    let other = window(state.path());
    let mut pending = Offer::send(state.path(), &source.window.id, None, tab()).unwrap();
    let first = transfers::receive(state.path(), pending.id(), &target.window.id).unwrap();
    let second = transfers::receive(state.path(), pending.id(), &other.window.id).unwrap();
    first.acknowledge(state.path(), &target.window.id).unwrap();
    assert!(second.acknowledge(state.path(), &other.window.id).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = state
            .path()
            .join("transfers")
            .join(format!("{}.json", pending.id()));
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        pending.poll().unwrap(),
        Status::Accepted {
            window: target.window.id.clone()
        }
    );
}
