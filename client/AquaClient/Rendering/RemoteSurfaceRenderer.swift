import UIKit

/// Swap point for the future remote surface pipeline.
///
///     Linux Wayland surface -> hardware encoding -> network
///         -> VideoToolbox -> RemoteSurfaceView
///
/// Phase 1 ships `PlaceholderSurfaceRenderer`. A later phase replaces it with a
/// Metal / `AVSampleBufferDisplayLayer` renderer **without touching**
/// `RemoteWindowViewController` or the window model.
@MainActor
protocol RemoteSurfaceRendering: AnyObject {
    /// The view that actually renders the remote surface.
    var view: UIView { get }

    /// The remote window this surface belongs to (nil until resolved).
    func update(remoteWindow: RemoteWindow?)

    /// The current viewport of the hosting scene.
    func update(viewport: RemoteViewport)

    /// The hosting scene session identifier, for diagnostics.
    func update(sceneSessionIdentifier: String?)
}
