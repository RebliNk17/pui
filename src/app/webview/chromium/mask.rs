//! Lets egui popups show through the native page instead of hiding it.
//!
//! Native CEF views composite above egui. When a floating egui surface (menu,
//! popup) overlaps the page, the page container's layer gets a mask with holes
//! at those rectangles, and hit-testing ignores the holes so clicks reach egui.
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::RefCell;
use std::ffi::c_void;

#[repr(C)]
pub(super) struct CGPath {
    _private: [u8; 0],
}
unsafe impl RefEncode for CGPath {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGPath", &[]));
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPathCreateMutable() -> *mut CGPath;
    fn CGPathAddRect(path: *mut CGPath, transform: *const c_void, rect: NSRect);
    fn CGPathRelease(path: *const CGPath);
}

#[derive(Default)]
pub(super) struct ContainerState {
    /// Holes in the container's own (bottom-left origin) coordinates.
    holes: RefCell<Vec<NSRect>>,
}

define_class! {
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "PuiBrowserContainer"]
    #[ivars = ContainerState]
    pub(super) struct Container;
    impl Container {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            if self.ivars().holes.borrow().iter().any(|hole| contains(*hole, local)) {
                return std::ptr::null_mut();
            }
            unsafe { msg_send![super(self), hitTest: point] }
        }
    }
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}

impl Container {
    pub(super) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContainerState::default());
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        this
    }

    /// `holes` are in container coordinates (points, bottom-left origin).
    pub(super) fn set_holes(&self, holes: Vec<NSRect>) {
        if *self.ivars().holes.borrow() == holes {
            return;
        }
        let layer: Option<Retained<AnyObject>> = unsafe { msg_send![self, layer] };
        let Some(layer) = layer else {
            return;
        };
        let layer: &AnyObject = &layer;
        let transaction = AnyClass::get(c"CATransaction");
        if let Some(transaction) = transaction {
            let _: () = unsafe { msg_send![transaction, begin] };
            let _: () = unsafe { msg_send![transaction, setDisableActions: Bool::YES] };
        }
        if holes.is_empty() {
            let none: Option<&AnyObject> = None;
            let _: () = unsafe { msg_send![layer, setMask: none] };
        } else if let Some(shape) = AnyClass::get(c"CAShapeLayer") {
            let bounds = self.bounds();
            let mask: Retained<AnyObject> = unsafe { msg_send![shape, layer] };
            let _: () = unsafe { msg_send![&*mask, setFrame: bounds] };
            unsafe {
                let path = CGPathCreateMutable();
                for rect in visible(bounds, &holes) {
                    CGPathAddRect(path, std::ptr::null(), rect);
                }
                let _: () = msg_send![&*mask, setPath: path as *const CGPath];
                CGPathRelease(path);
            }
            let _: () = unsafe { msg_send![layer, setMask: &*mask] };
        }
        if let Some(transaction) = transaction {
            let _: () = unsafe { msg_send![transaction, commit] };
        }
        *self.ivars().holes.borrow_mut() = holes;
    }
}

/// Non-overlapping rectangles covering `bounds` minus every hole.
fn visible(bounds: NSRect, holes: &[NSRect]) -> Vec<NSRect> {
    let mut parts = vec![bounds];
    for hole in holes {
        parts = parts
            .into_iter()
            .flat_map(|part| subtract(part, *hole))
            .collect();
    }
    parts
}

fn subtract(rect: NSRect, hole: NSRect) -> Vec<NSRect> {
    let (rx0, ry0) = (rect.origin.x, rect.origin.y);
    let (rx1, ry1) = (rx0 + rect.size.width, ry0 + rect.size.height);
    let hx0 = hole.origin.x.max(rx0);
    let hy0 = hole.origin.y.max(ry0);
    let hx1 = (hole.origin.x + hole.size.width).min(rx1);
    let hy1 = (hole.origin.y + hole.size.height).min(ry1);
    if hx0 >= hx1 || hy0 >= hy1 {
        return vec![rect];
    }
    let make = |x0: f64, y0: f64, x1: f64, y1: f64| {
        NSRect::new(NSPoint::new(x0, y0), NSSize::new(x1 - x0, y1 - y0))
    };
    let mut out = Vec::with_capacity(4);
    if hy0 > ry0 {
        out.push(make(rx0, ry0, rx1, hy0));
    }
    if hy1 < ry1 {
        out.push(make(rx0, hy1, rx1, ry1));
    }
    if hx0 > rx0 {
        out.push(make(rx0, hy0, hx0, hy1));
    }
    if hx1 < rx1 {
        out.push(make(hx1, hy0, rx1, hy1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }
    fn area(rects: &[NSRect]) -> f64 {
        rects.iter().map(|r| r.size.width * r.size.height).sum()
    }

    #[test]
    fn subtracting_holes_keeps_the_rest_without_overlap() {
        let bounds = r(0.0, 0.0, 100.0, 100.0);
        assert_eq!(area(&visible(bounds, &[])), 10_000.0);
        assert_eq!(
            area(&visible(bounds, &[r(10.0, 10.0, 20.0, 20.0)])),
            9_600.0
        );
        // Overlapping holes are not double counted; outside holes are ignored.
        let parts = visible(
            bounds,
            &[
                r(10.0, 10.0, 20.0, 20.0),
                r(20.0, 20.0, 20.0, 20.0),
                r(200.0, 0.0, 5.0, 5.0),
            ],
        );
        assert_eq!(area(&parts), 10_000.0 - 400.0 - 400.0 + 100.0);
        assert!(visible(bounds, &[r(-5.0, -5.0, 200.0, 200.0)]).is_empty());
    }
}
