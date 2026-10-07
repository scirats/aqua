//! Integration tests for the pure Aqua domain registry.
//!
//! These run without any Wayland compositor: the registry is driven with opaque
//! surface/client tokens, exactly like the Wayland adapter does at runtime.

use aqua_server::domain::{
    ClientKey, RemoteApplicationId, RemoteEvent, RemoteSurfaceId, RemoteViewport, RemoteWindowId,
    RemoteWindowState, WindowRegistry,
};

fn client(n: u64) -> ClientKey {
    ClientKey(n)
}

fn surface(n: u64) -> RemoteSurfaceId {
    RemoteSurfaceId(n)
}

/// 1. Multiple xdg_toplevels of the same application produce distinct windows.
#[test]
fn multiple_toplevels_same_app_are_distinct_windows() {
    let mut registry = WindowRegistry::new();
    let events_a = registry.create_toplevel(client(1), surface(1));
    let events_b = registry.create_toplevel(client(1), surface(2));

    let id_a = match &events_a[0] {
        RemoteEvent::WindowCreated { window } => window.id.clone(),
        other => panic!("unexpected: {other:?}"),
    };
    let id_b = match &events_b[0] {
        RemoteEvent::WindowCreated { window } => window.id.clone(),
        other => panic!("unexpected: {other:?}"),
    };

    assert_ne!(id_a, id_b);
    assert_eq!(registry.window_count(), 2);

    // Same application id can be shared.
    registry.update_app_id(
        surface(1),
        Some(RemoteApplicationId::new("org.mozilla.firefox")),
    );
    registry.update_app_id(
        surface(2),
        Some(RemoteApplicationId::new("org.mozilla.firefox")),
    );
    assert_eq!(
        registry.window(&id_a).unwrap().application_id,
        registry.window(&id_b).unwrap().application_id
    );
}

/// 2. Titles update, and 7. the window id does not depend on the title.
#[test]
fn titles_update_without_changing_identity() {
    let mut registry = WindowRegistry::new();
    let id = create(&mut registry, 1);

    let events = registry.update_title(surface(1), Some("Mozilla Firefox".into()));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind(), "window.title_changed");

    let second = registry.update_title(surface(1), Some("Firefox — New Tab".into()));
    assert_eq!(second.len(), 1);
    assert_eq!(
        registry.window(&id).unwrap().title.as_deref(),
        Some("Firefox — New Tab")
    );

    // Same title again -> no event.
    assert!(registry
        .update_title(surface(1), Some("Firefox — New Tab".into()))
        .is_empty());

    // Identity is untouched by title changes.
    assert_eq!(registry.window(&id).unwrap().id, id);
}

/// 3. app_id maps to the application id.
#[test]
fn app_id_maps_to_application_id() {
    let mut registry = WindowRegistry::new();
    let id = create(&mut registry, 1);

    let events = registry.update_app_id(
        surface(1),
        Some(RemoteApplicationId::new("org.gnome.Terminal")),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind(), "window.app_id_changed");
    assert_eq!(
        registry
            .window(&id)
            .unwrap()
            .application_id
            .as_ref()
            .unwrap()
            .as_str(),
        "org.gnome.Terminal"
    );

    // An empty app id clears it.
    let cleared = registry.update_app_id(surface(1), None);
    assert_eq!(cleared.len(), 1);
    assert!(registry.window(&id).unwrap().application_id.is_none());
}

/// 4. Destroy produces WindowClosed exactly once.
#[test]
fn destroy_emits_window_closed_once() {
    let mut registry = WindowRegistry::new();
    create(&mut registry, 1);

    let first = registry.destroy_toplevel(surface(1));
    let closed = first.iter().filter(|e| e.kind() == "window.closed").count();
    assert_eq!(closed, 1);
    assert_eq!(registry.window_count(), 0);

    let second = registry.destroy_toplevel(surface(1));
    assert!(second.iter().all(|e| e.kind() != "window.closed"));
    assert!(second.is_empty());
}

/// 5. A popup never creates a RemoteWindow.
#[test]
fn popup_does_not_create_window() {
    let mut registry = WindowRegistry::new();
    let window = create(&mut registry, 1);

    let events = registry.register_popup(client(1), surface(10), surface(1));
    assert!(events.iter().any(|e| e.kind() == "popup.created"));
    assert!(events.iter().all(|e| e.kind() != "window.created"));
    assert_eq!(registry.window_count(), 1);

    // The popup belongs to the window's tree.
    assert_eq!(registry.window_for_surface(surface(10)), Some(window));
    assert_eq!(
        registry.surface_kind(surface(10)).unwrap().as_str(),
        "popup"
    );
}

/// 6. A subsurface never creates a RemoteWindow.
#[test]
fn subsurface_does_not_create_window() {
    let mut registry = WindowRegistry::new();
    let window = create(&mut registry, 1);

    let events = registry.register_subsurface(client(1), surface(11), surface(1));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind(), "surface.created");
    assert_eq!(registry.window_count(), 1);
    assert_eq!(registry.window_for_surface(surface(11)), Some(window));
}

/// 8. A viewport configure keeps the window identity.
#[test]
fn viewport_change_keeps_identity() {
    let mut registry = WindowRegistry::new();
    let id = create(&mut registry, 1);

    let events = registry.viewport_changed(id.clone(), RemoteViewport::new(800, 600, 1.0));
    assert_eq!(events.len(), 1);
    match &events[0] {
        RemoteEvent::ViewportChanged {
            id: event_id,
            viewport,
        } => {
            assert_eq!(event_id, &id);
            assert_eq!(viewport.width, 800);
            assert_eq!(viewport.height, 600);
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert_eq!(registry.window(&id).unwrap().id, id);
}

/// 9. Client disconnect cleans up all of its windows exactly once.
#[test]
fn client_disconnect_closes_its_windows() {
    let mut registry = WindowRegistry::new();
    create(&mut registry, 1); // client 1
    create(&mut registry, 2); // client 1
    let other = {
        // A different client with its own window.
        let events = registry.create_toplevel(client(2), surface(9));
        match &events[0] {
            RemoteEvent::WindowCreated { window } => window.id.clone(),
            other => panic!("unexpected: {other:?}"),
        }
    };

    let events = registry.client_disconnected(client(1));
    let closed = events
        .iter()
        .filter(|e| e.kind() == "window.closed")
        .count();
    assert_eq!(closed, 2);
    assert_eq!(registry.window_count(), 1);
    assert!(registry.window(&other).is_some());

    // Second disconnect is a no-op.
    assert!(registry.client_disconnected(client(1)).is_empty());
}

/// 10. Surface lifecycle is not confused with window lifecycle.
#[test]
fn surface_lifecycle_is_not_window_lifecycle() {
    let mut registry = WindowRegistry::new();
    let window = create(&mut registry, 1);

    // Main surface commit does not create or close a window.
    let commits = registry.surface_committed(surface(1), None);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].kind(), "surface.commit");
    assert_eq!(registry.window_count(), 1);

    // A subsurface commit does not create or close a window either.
    registry.register_subsurface(client(1), surface(2), surface(1));
    registry.surface_committed(surface(2), None);
    assert_eq!(registry.window_count(), 1);

    // Destroying a popup does not close the window.
    registry.register_popup(client(1), surface(3), surface(1));
    let popup_events = registry.destroy_popup(surface(3));
    assert!(popup_events.iter().any(|e| e.kind() == "popup.destroyed"));
    assert!(popup_events.iter().all(|e| e.kind() != "window.closed"));
    assert_eq!(registry.window_count(), 1);
    assert_eq!(registry.window(&window).unwrap().id, window);
}

/// State transitions map cleanly for maximized/fullscreen and are idempotent.
#[test]
fn window_state_changes() {
    let mut registry = WindowRegistry::new();
    let id = create(&mut registry, 1);

    let events = registry.update_state(surface(1), RemoteWindowState::Maximized);
    assert_eq!(events[0].kind(), "window.state_changed");
    assert_eq!(
        registry.window(&id).unwrap().state,
        RemoteWindowState::Maximized
    );

    assert!(registry
        .update_state(surface(1), RemoteWindowState::Maximized)
        .is_empty());

    let events = registry.update_state(surface(1), RemoteWindowState::Fullscreen);
    assert_eq!(events[0].kind(), "window.state_changed");
}

fn create(registry: &mut WindowRegistry, surface_number: u64) -> RemoteWindowId {
    let events = registry.create_toplevel(client(1), surface(surface_number));
    match &events[0] {
        RemoteEvent::WindowCreated { window } => window.id.clone(),
        other => panic!("unexpected: {other:?}"),
    }
}
