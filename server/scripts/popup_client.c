// Minimal Wayland client used to prove that Aqua treats an `xdg_popup` as a
// surface of an existing window, never as a new RemoteWindow.
//
// It creates one toplevel ("popup-demo"), completes the configure/ack handshake,
// then creates an xdg_popup parented to that toplevel. No GPU, no dmabuf, no
// shm buffers are needed to exercise the popup lifecycle.
//
// Build (inside the Linux dev container):
//   wayland-scanner client-header xdg-shell.xml xdg-shell-client-protocol.h
//   wayland-scanner private-code  xdg-shell.xml xdg-shell-protocol.c
//   cc popup_client.c xdg-shell-protocol.c $(pkg-config --cflags --libs wayland-client) -o popup_client

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

struct client {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct xdg_wm_base *wm_base;

    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *toplevel;

    struct wl_surface *popup_surface;
    struct xdg_surface *popup_xdg_surface;
    struct xdg_popup *popup;

    int toplevel_configured;
    int popup_configured;
};

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version) {
    struct client *c = data;
    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        c->compositor = wl_registry_bind(registry, name, &wl_compositor_interface,
                                         version < 4 ? version : 4);
    } else if (strcmp(interface, xdg_wm_base_interface.name) == 0) {
        c->wm_base = wl_registry_bind(registry, name, &xdg_wm_base_interface, version);
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry, uint32_t name) {}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

static void wm_base_ping(void *data, struct xdg_wm_base *wm_base, uint32_t serial) {
    xdg_wm_base_pong(wm_base, serial);
}

static const struct xdg_wm_base_listener wm_base_listener = { .ping = wm_base_ping };

static void xdg_surface_configure(void *data, struct xdg_surface *xdg_surface, uint32_t serial) {
    struct client *c = data;
    xdg_surface_ack_configure(xdg_surface, serial);
    if (xdg_surface == c->xdg_surface) {
        c->toplevel_configured = 1;
    } else if (xdg_surface == c->popup_xdg_surface) {
        c->popup_configured = 1;
    }
}

static const struct xdg_surface_listener xdg_surface_listener = { .configure = xdg_surface_configure };

static void popup_configure(void *data, struct xdg_popup *popup, int32_t x, int32_t y,
                            int32_t width, int32_t height) {}
static void popup_done(void *data, struct xdg_popup *popup) {}

static const struct xdg_popup_listener popup_listener = {
    .configure = popup_configure,
    .popup_done = popup_done,
};

static void pump(struct client *c) {
    if (wl_display_dispatch(c->display) == -1) {
        fprintf(stderr, "popup-client: display dispatch failed\n");
        exit(2);
    }
}

int main(void) {
    struct client c;
    memset(&c, 0, sizeof c);

    c.display = wl_display_connect(NULL);
    if (!c.display) {
        fprintf(stderr, "popup-client: cannot connect to Wayland display\n");
        return 1;
    }

    c.registry = wl_display_get_registry(c.display);
    wl_registry_add_listener(c.registry, &registry_listener, &c);
    wl_display_roundtrip(c.display);

    if (!c.compositor || !c.wm_base) {
        fprintf(stderr, "popup-client: missing wl_compositor or xdg_wm_base\n");
        return 1;
    }
    xdg_wm_base_add_listener(c.wm_base, &wm_base_listener, &c);

    /* --- Toplevel ------------------------------------------------------- */
    c.surface = wl_compositor_create_surface(c.compositor);
    c.xdg_surface = xdg_wm_base_get_xdg_surface(c.wm_base, c.surface);
    xdg_surface_add_listener(c.xdg_surface, &xdg_surface_listener, &c);
    c.toplevel = xdg_surface_get_toplevel(c.xdg_surface);
    xdg_toplevel_set_title(c.toplevel, "popup-demo");
    xdg_toplevel_set_app_id(c.toplevel, "org.aqua.popup-demo");
    wl_surface_commit(c.surface);

    while (!c.toplevel_configured) {
        pump(&c);
    }

    /* --- Popup parented to the toplevel --------------------------------- */
    struct xdg_positioner *positioner = xdg_wm_base_create_positioner(c.wm_base);
    xdg_positioner_set_size(positioner, 200, 120);
    xdg_positioner_set_anchor_rect(positioner, 10, 10, 1, 1);
    xdg_positioner_set_anchor(positioner, XDG_POSITIONER_ANCHOR_BOTTOM_RIGHT);
    xdg_positioner_set_gravity(positioner, XDG_POSITIONER_GRAVITY_BOTTOM_RIGHT);
    xdg_positioner_set_constraint_adjustment(
        positioner,
        XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_SLIDE_X |
            XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_SLIDE_Y);

    c.popup_surface = wl_compositor_create_surface(c.compositor);
    c.popup_xdg_surface = xdg_wm_base_get_xdg_surface(c.wm_base, c.popup_surface);
    xdg_surface_add_listener(c.popup_xdg_surface, &xdg_surface_listener, &c);
    c.popup = xdg_surface_get_popup(c.popup_xdg_surface, c.xdg_surface, positioner);
    xdg_popup_add_listener(c.popup, &popup_listener, &c);
    xdg_positioner_destroy(positioner);
    wl_surface_commit(c.popup_surface);

    while (!c.popup_configured) {
        pump(&c);
    }

    fprintf(stderr, "popup-client: toplevel + popup mapped, idling\n");
    fflush(stderr);

    /* Stay alive briefly so the compositor observes the popup, then exit. */
    for (int i = 0; i < 40; i++) {
        if (wl_display_dispatch_pending(c.display) == -1) {
            break;
        }
        wl_display_flush(c.display);
        usleep(50 * 1000);
    }

    xdg_popup_destroy(c.popup);
    xdg_surface_destroy(c.popup_xdg_surface);
    wl_surface_destroy(c.popup_surface);
    xdg_toplevel_destroy(c.toplevel);
    xdg_surface_destroy(c.xdg_surface);
    wl_surface_destroy(c.surface);
    xdg_wm_base_destroy(c.wm_base);
    wl_registry_destroy(c.registry);
    wl_display_disconnect(c.display);
    return 0;
}
