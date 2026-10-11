//! Run with `cargo test -p heron-eq --test native_scale -- --ignored` on Retina macOS.
//! A custom harness keeps AppKit on the process main thread. The test is opt-in
//! because it needs an interactive desktop, Retina display, and a Metal device.

fn main() {
    #[cfg(target_os = "macos")]
    if std::env::args().any(|arg| arg == "--ignored") {
        macos::eq_uses_retina_backing_pixels();
        return;
    }
    println!("native_scale: ignored (requires an interactive Retina macOS session)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSView,
        NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};
    use objc2_quartz_core::{CALayer, CAMetalLayer};
    use raw_window_handle_05::{AppKitWindowHandle, HasRawWindowHandle};
    use truce::{
        core::editor::{RawWindowHandle, for_test_params},
        prelude::*,
    };

    pub(super) fn eq_uses_retina_backing_pixels() {
        let mtm = MainThreadMarker::new().expect("native test must run on the process main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.finishLaunching();
        let plugin = heron_eq::Plugin::create();
        let params = plugin.params_arc();
        let mut editor = plugin.editor_builder()(Arc::clone(&params)).expect("EQ editor");
        let (width, height) = editor.size();
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(width), f64::from(height)),
        );
        // SAFETY: all AppKit objects are owned and used on this process main
        // thread, and retained until after the editor closes below.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: Rust retains the window and releases it after editor teardown.
        unsafe { window.setReleasedWhenClosed(false) };
        let parent = NSView::initWithFrame(NSView::alloc(mtm), frame);
        window.setContentView(Some(&parent));
        window.center();
        window.makeKeyAndOrderFront(None);
        app.activate();
        let backing = window.backingScaleFactor();
        assert!(backing > 1.0, "run this regression on a Retina display");
        verify_native_backing_notification(&parent, backing);

        // VST3 host content-scale callbacks can announce 1x independently of
        // AppKit's Retina backing pixels. The native view remains point-sized.
        editor.set_scale_factor(1.0);
        let handle = RawWindowHandle::AppKit((&*parent as *const NSView).cast_mut().cast());
        editor.open(handle, for_test_params(params));
        let child = parent.subviews().objectAtIndex(0);
        let expected = NSSize::new(f64::from(width) * backing, f64::from(height) * backing);
        wait_for_drawable(&child, expected, "initial attach after host announces 1x");

        editor.set_scale_factor(1.0);
        pump_for(Duration::from_millis(150));
        wait_for_drawable(
            &child,
            expected,
            "late host scale must retain Retina backing pixels",
        );

        editor.set_scale_factor(1.25);
        pump_for(Duration::from_millis(150));
        wait_for_drawable(
            &child,
            expected,
            "host 1.25x zoom must not replace backing density",
        );
        assert!(editor.set_size(1280, 820));
        window.setContentSize(NSSize::new(1280.0, 820.0));
        wait_for_drawable(
            &child,
            NSSize::new(1280.0 * backing, 820.0 * backing),
            "resizing after a host zoom callback must retain Retina backing pixels",
        );

        editor.close();
        window.close();
        println!(
            "native_scale: EQ logical {width}x{height}, backing {backing}x, drawable {}x{}",
            expected.width, expected.height
        );
    }

    // A distinct boundary from the Iced surface: baseview must tell its handler
    // the actual backing density after attach, so moving between displays can
    // update the live viewport without an unrelated logical frame resize.
    fn verify_native_backing_notification(parent: &NSView, backing: f64) {
        struct Parent<'a>(&'a NSView);
        // SAFETY: the borrowed parent outlives the child, on its AppKit thread.
        unsafe impl HasRawWindowHandle for Parent<'_> {
            fn raw_window_handle(&self) -> raw_window_handle_05::RawWindowHandle {
                let mut handle = AppKitWindowHandle::empty();
                handle.ns_view = (self.0 as *const NSView).cast_mut().cast();
                handle.into()
            }
        }
        struct Probe(Arc<Mutex<Option<baseview::WindowInfo>>>);
        impl baseview::WindowHandler for Probe {
            fn on_frame(&mut self, _: &mut baseview::Window) {}
            fn on_event(
                &mut self,
                _: &mut baseview::Window,
                event: baseview::Event,
            ) -> baseview::EventStatus {
                if let baseview::Event::Window(baseview::WindowEvent::Resized(info)) = event {
                    *self.0.lock().unwrap() = Some(info);
                }
                baseview::EventStatus::Ignored
            }
        }
        let observed = Arc::new(Mutex::new(None));
        let captured = Arc::clone(&observed);
        let mut child = baseview::Window::open_parented(
            &Parent(parent),
            baseview::WindowOpenOptions::new().with_size(320.0, 200.0),
            move |_| Probe(captured),
        );
        pump_for(Duration::from_millis(30));
        let info = observed
            .lock()
            .unwrap()
            .expect("native attach must deliver the backing-size notification");
        assert_eq!(info.scale(), backing);
        assert_eq!(f64::from(info.physical_size().width), 320.0 * backing);
        assert_eq!(f64::from(info.physical_size().height), 200.0 * backing);
        child.close();
    }

    fn wait_for_drawable(view: &NSView, expected: NSSize, stage: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(actual) = view.layer().and_then(|layer| metal_drawable_size(&layer)) {
                if actual == expected {
                    println!(
                        "native_scale: {stage}: {}x{} pixels",
                        actual.width, actual.height
                    );
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "{stage}: drawable {actual:?}, expected {expected:?}"
                );
            } else {
                assert!(
                    Instant::now() < deadline,
                    "{stage}: editor must create a Metal layer"
                );
            }
            pump_for(Duration::from_millis(20));
        }
    }

    fn metal_drawable_size(layer: &CALayer) -> Option<NSSize> {
        if let Some(metal) = layer.downcast_ref::<CAMetalLayer>() {
            return Some(metal.drawableSize());
        }
        // SAFETY: this is the editor's retained, main-thread-owned layer tree.
        unsafe { layer.sublayers() }
            .and_then(|layers| layers.iter().find_map(|layer| metal_drawable_size(&layer)))
    }

    fn pump_for(duration: Duration) {
        let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
        let until = Instant::now() + duration;
        let deadline = NSDate::dateWithTimeIntervalSinceNow(duration.as_secs_f64());
        while Instant::now() < until {
            // SAFETY: Foundation's immutable default-mode constant remains live.
            let mode = unsafe { NSDefaultRunLoopMode };
            if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&deadline),
                mode,
                true,
            ) {
                app.sendEvent(&event);
            }
            app.updateWindows();
        }
    }
}
