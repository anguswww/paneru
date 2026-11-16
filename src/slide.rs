//! The snapshot slide behind the "rows" workspace animation.
//!
//! Moving real windows vertically cannot animate a workspace switch cleanly:
//! macOS keeps title bars below the top display's menu bar, a display stacked
//! below adopts windows that cross into it, and every frame costs an AX round
//! trip per window (which slow apps answer late). Instead the display is
//! captured before the switch and covered with that image, the real windows
//! switch instantly underneath, and Core Animation slides the image away on the
//! window server, uncovering the new workspace live.

use bevy::math::IRect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
#[allow(deprecated, reason = "ScreenCaptureKit has no synchronous capture")]
use objc2_core_graphics::CGWindowListCreateImage;
use objc2_core_graphics::{
    CGImage, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess, CGWindowID,
    CGWindowImageOption, CGWindowListOption,
};
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{
    CABasicAnimation, CALayer, CAMediaTiming as _, CAMediaTimingFunction, CATransaction,
};
use tracing::debug;

use crate::overlay::{cg_abs_to_cocoa, primary_screen_height};
use crate::platform::WinID;

/// Whether this process may capture other apps' windows. Without it a capture
/// shows only the wallpaper, so the slide is skipped instead.
pub fn screen_capture_allowed() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// Asks macOS for Screen Recording permission. It prompts at most once per
/// binary; afterwards the user has to grant it in System Settings.
pub fn request_screen_capture() {
    if !CGRequestScreenCaptureAccess() {
        debug!("screen recording not granted; workspace slides fall back to instant switches");
    }
}

/// Captures what is on screen inside `rect` (absolute CG coordinates). With
/// `below`, only windows underneath that window are included, so a capture
/// taken while the slide overlay is up shows the real windows under it.
pub fn capture(rect: IRect, below: Option<WinID>) -> Option<CFRetained<CGImage>> {
    let bounds = CGRect::new(
        CGPoint::new(f64::from(rect.min.x), f64::from(rect.min.y)),
        CGSize::new(f64::from(rect.width()), f64::from(rect.height())),
    );
    let (list, window) = match below.and_then(|id| CGWindowID::try_from(id).ok()) {
        Some(id) => (CGWindowListOption::OptionOnScreenBelowWindow, id),
        None => (CGWindowListOption::OptionOnScreenOnly, 0),
    };
    #[allow(deprecated, reason = "ScreenCaptureKit has no synchronous capture")]
    CGWindowListCreateImage(
        bounds,
        list,
        window,
        CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution,
    )
}

/// The overlay window and the image layer that slides inside it.
pub struct WorkspaceSlideOverlay {
    mtm: MainThreadMarker,
    window: Option<Retained<NSWindow>>,
    image: Retained<CALayer>,
    /// The covered rect, in absolute CG coordinates.
    rect: IRect,
    visible: bool,
}

impl WorkspaceSlideOverlay {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self {
            mtm,
            window: None,
            image: CALayer::new(),
            rect: IRect::default(),
            visible: false,
        }
    }

    /// The overlay's window number while it is on screen, to capture below it.
    pub fn window_id(&self) -> Option<WinID> {
        self.window
            .as_ref()
            .filter(|_| self.visible)
            .and_then(|window| WinID::try_from(window.windowNumber()).ok())
    }

    /// Covers `rect` with `image`, the display as it looks before the switch,
    /// so the real windows can be rearranged out of sight.
    pub fn cover(&mut self, rect: IRect, image: &CGImage) {
        let window = self.window_for(rect);
        let size = self.size();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.image.removeAllAnimations();
        set_image(&self.image, Some(image));
        self.image
            .setFrame(CGRect::new(CGPoint::new(0.0, 0.0), size));
        CATransaction::commit();
        window.orderFrontRegardless();
        self.visible = true;
        // Push the cover to the window server now: the windows underneath move
        // later this frame, and must not be seen doing so.
        window.displayIfNeeded();
        CATransaction::flush();
    }

    /// Slides the cover off the display, up when `upwards`, uncovering the
    /// real windows beneath it.
    pub fn slide_away(&mut self, upwards: bool, duration: f64) {
        let height = self.size().height;
        let centre = height / 2.0;
        // Layer-backed views are not flipped: +y is up.
        let target = if upwards {
            centre + height
        } else {
            centre - height
        };

        let animation =
            CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("position.y")));
        let from = NSNumber::new_f64(centre);
        let to = NSNumber::new_f64(target);
        // SAFETY: `position.y` animates between `NSNumber`s.
        unsafe {
            animation.setFromValue(Some(&from));
            animation.setToValue(Some(&to));
        }
        animation.setDuration(duration);
        animation.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
            0.3, 0.0, 0.2, 1.0,
        )));

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let position = self.image.position();
        self.image.setPosition(CGPoint::new(position.x, target));
        self.image
            .addAnimation_forKey(&animation, Some(&NSString::from_str("slide")));
        CATransaction::commit();
        CATransaction::flush();
    }

    /// Takes the overlay down and drops the image.
    pub fn hide(&mut self) {
        if let Some(window) = &self.window {
            window.orderOut(None::<&AnyObject>);
        }
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.image.removeAllAnimations();
        set_image(&self.image, None);
        CATransaction::commit();
        self.visible = false;
    }

    fn size(&self) -> CGSize {
        CGSize::new(f64::from(self.rect.width()), f64::from(self.rect.height()))
    }

    /// The overlay window, created on first use and moved over `rect`.
    fn window_for(&mut self, rect: IRect) -> Retained<NSWindow> {
        self.rect = rect;
        let cg = NSRect::new(
            NSPoint::new(f64::from(rect.min.x), f64::from(rect.min.y)),
            NSSize::new(f64::from(rect.width()), f64::from(rect.height())),
        );
        let frame = cg_abs_to_cocoa(cg, primary_screen_height(self.mtm));
        if let Some(window) = &self.window {
            window.setFrame_display(frame, false);
            return window.clone();
        }
        let window = make_slide_window(self.mtm, frame);
        if let Some(view) = window.contentView() {
            view.setWantsLayer(true);
            if let Some(root) = view.layer() {
                root.setMasksToBounds(true);
                root.addSublayer(&self.image);
            }
        }
        self.window = Some(window.clone());
        window
    }
}

fn set_image(layer: &CALayer, image: Option<&CGImage>) {
    let contents = image.map(|image| {
        // SAFETY: a `CGImage` is a CF object, toll-free usable as layer contents.
        unsafe { &*std::ptr::from_ref(image).cast::<AnyObject>() }
    });
    // SAFETY: `contents` is a `CGImage` or nil, both valid layer contents.
    unsafe { layer.setContents(contents) };
}

fn make_slide_window(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // Releasing on close would free the window under our `Retained`.
    unsafe { window.setReleasedWhenClosed(false) };
    // Transparent: once the image starts moving, the real (already switched)
    // windows show through where it no longer covers.
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setIgnoresMouseEvents(true);
    window.setHasShadow(false);
    // Above Paneru's dim and border overlays, which the captures include.
    window.setLevel(NSFloatingWindowLevel + 1);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenNone,
    );
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), frame.size),
    );
    window.setContentView(Some(&view));
    window
}
