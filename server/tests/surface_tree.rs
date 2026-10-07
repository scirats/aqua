//! Surface-tree tests: subsurfaces and popups belong to a window, never create
//! one, and geometry updates emit `surface.updated`.

use aqua_server::domain::{
    ClientKey, RemoteEvent, RemoteSurfaceId, RemoteSurfaceKind, WindowRegistry,
};

fn client(n: u64) -> ClientKey {
    ClientKey(n)
}

fn surface(n: u64) -> RemoteSurfaceId {
    RemoteSurfaceId(n)
}

fn root_window(registry: &mut WindowRegistry) -> aqua_server::domain::RemoteWindowId {
    let events = registry.create_toplevel(client(1), surface(1));
    match &events[0] {
        RemoteEvent::WindowCreated { window } => window.id.clone(),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn subsurface_shares_window_and_parent() {
    let mut registry = WindowRegistry::new();
    let window = root_window(&mut registry);

    registry.register_subsurface(client(1), surface(2), surface(1));
    let subsurf = registry.surface(surface(2)).expect("subsurface");
    assert_eq!(subsurf.window, window);
    assert_eq!(subsurf.parent, Some(surface(1)));
    assert_eq!(subsurf.kind, RemoteSurfaceKind::Subsurface);
    assert_eq!(registry.window_count(), 1);
}

#[test]
fn popup_shares_window_and_never_creates_one() {
    let mut registry = WindowRegistry::new();
    let window = root_window(&mut registry);

    registry.register_popup(client(1), surface(3), surface(1));
    let popup = registry.surface(surface(3)).expect("popup");
    assert_eq!(popup.window, window);
    assert_eq!(popup.parent, Some(surface(1)));
    assert_eq!(popup.kind, RemoteSurfaceKind::Popup);
    assert_eq!(registry.window_count(), 1);
}

#[test]
fn surface_created_event_carries_surface_info() {
    let mut registry = WindowRegistry::new();
    let events = registry.create_toplevel(client(1), surface(1));
    let InfoCheck {
        id,
        window,
        parent,
        kind,
    } = match &events[1] {
        RemoteEvent::SurfaceCreated { surface } => InfoCheck {
            id: surface.id,
            window: surface.window.clone(),
            parent: surface.parent,
            kind: surface.kind,
        },
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(id, surface(1));
    assert_eq!(parent, None);
    assert_eq!(kind, RemoteSurfaceKind::Toplevel);
    assert_eq!(
        Some(window),
        registry.surface(surface(1)).map(|s| s.window.clone())
    );
}

struct InfoCheck {
    id: RemoteSurfaceId,
    window: aqua_server::domain::RemoteWindowId,
    parent: Option<RemoteSurfaceId>,
    kind: RemoteSurfaceKind,
}

#[test]
fn geometry_update_emits_surface_updated() {
    let mut registry = WindowRegistry::new();
    root_window(&mut registry);
    registry.register_subsurface(client(1), surface(2), surface(1));

    let events = registry.update_surface_geometry(surface(2), (10, 20), (50, 40), 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind(), "surface.updated");
    match &events[0] {
        RemoteEvent::SurfaceUpdated { surface } => {
            assert_eq!(surface.position, (10, 20));
            assert_eq!(surface.size, (50, 40));
            assert_eq!(surface.z, 1);
        }
        other => panic!("unexpected {other:?}"),
    }

    // No change -> no event.
    assert!(registry
        .update_surface_geometry(surface(2), (10, 20), (50, 40), 1)
        .is_empty());
}

#[test]
fn window_surfaces_orders_root_first() {
    let mut registry = WindowRegistry::new();
    root_window(&mut registry);
    registry.register_subsurface(client(1), surface(2), surface(1));
    registry.register_popup(client(1), surface(3), surface(1));
    let window = registry.surface(surface(1)).unwrap().window.clone();

    let surfaces = registry.window_surfaces(&window);
    assert_eq!(surfaces.len(), 3);
    assert_eq!(surfaces[0].id, surface(1));
    assert_eq!(surfaces[0].kind, RemoteSurfaceKind::Toplevel);
}

#[test]
fn surface_destroyed_event_on_popup_close() {
    let mut registry = WindowRegistry::new();
    root_window(&mut registry);
    registry.register_popup(client(1), surface(3), surface(1));

    let events = registry.destroy_popup(surface(3));
    assert!(events.iter().any(|e| e.kind() == "popup.destroyed"));
    assert!(events.iter().any(|e| e.kind() == "surface.destroyed"));
    assert!(events.iter().all(|e| e.kind() != "window.closed"));
    assert_eq!(registry.surface_count(), 1);
}

#[test]
fn closing_window_releases_all_surfaces() {
    let mut registry = WindowRegistry::new();
    root_window(&mut registry);
    registry.register_subsurface(client(1), surface(2), surface(1));
    registry.register_popup(client(1), surface(3), surface(1));

    let events = registry.destroy_toplevel(surface(1));
    let destroyed = events
        .iter()
        .filter(|e| e.kind() == "surface.destroyed")
        .count();
    assert_eq!(destroyed, 3);
    assert!(events.iter().any(|e| e.kind() == "window.closed"));
    assert_eq!(registry.surface_count(), 0);
}
