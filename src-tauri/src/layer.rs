//! Layer-shell surfaces float above all windows and are never tiled. Compositors without
//! layer-shell (GNOME) get plain windows instead.

use gtk::cairo::{RectangleInt, Region};
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// Bottom centre, above everything, without keyboard or pointer input.
pub fn overlay(window: &gtk::ApplicationWindow, width: i32, height: i32) {
    if !init(window, "eckermann", width, height) {
        return;
    }
    window.set_layer(Layer::Overlay);
    // no margin: the page rises from the screen edge, and its padding sets the gap
    window.set_anchor(Edge::Bottom, true);
    window.set_keyboard_mode(KeyboardMode::None);
    // empty input region: clicks fall through to the window below
    window.input_shape_combine_region(Some(&Region::create()));
}

/// Centred panel that takes the keyboard when clicked. `margin` is the transparent
/// border that holds the shadow; clicks there fall through.
pub fn panel(window: &gtk::ApplicationWindow, width: i32, height: i32, margin: i32) {
    if !init(window, "eckermann-settings", width, height) {
        return;
    }
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    let inner = RectangleInt::new(margin, margin, width - 2 * margin, height - 2 * margin);
    window.input_shape_combine_region(Some(&Region::create_rectangle(&inner)));
}

fn init(window: &gtk::ApplicationWindow, namespace: &str, width: i32, height: i32) -> bool {
    if !gtk_layer_shell::is_supported() {
        log::warn!("compositor lacks layer-shell; {namespace} is a plain window");
        return false;
    }
    if window.is_realized() {
        log::warn!("{namespace} realized before layer-shell setup; it is a plain window");
        return false;
    }
    window.init_layer_shell();
    window.set_namespace(namespace);
    // a layer surface takes its content's natural size, which for a webview is near zero
    window.set_size_request(width, height);
    true
}
