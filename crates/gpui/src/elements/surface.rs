use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId, ObjectFit,
    Pixels, Style, StyleRefinement, Styled, Window,
};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2_core_foundation::CFRetained;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2_core_video::{CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use refineable::Refineable;

/// A source of a surface's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceSource {
    /// A CoreVideo image buffer
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    Surface(CFRetained<CVPixelBuffer>),
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl From<CFRetained<CVPixelBuffer>> for SurfaceSource {
    fn from(value: CFRetained<CVPixelBuffer>) -> Self {
        SurfaceSource::Surface(value)
    }
}

/// A surface element.
pub struct Surface {
    source: SurfaceSource,
    object_fit: ObjectFit,
    style: StyleRefinement,
}

/// Create a new surface element.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn surface(source: impl Into<SurfaceSource>) -> Surface {
    Surface {
        source: source.into(),
        object_fit: ObjectFit::Contain,
        style: Default::default(),
    }
}

impl Surface {
    /// Set the object fit for the image.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "ios")),
            allow(unused_variables)
        )]
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "ios")),
            allow(unused_variables)
        )]
        window: &mut Window,
        _: &mut App,
    ) {
        match &self.source {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            SurfaceSource::Surface(surface) => {
                let size = crate::size(
                    CVPixelBufferGetWidth(surface).into(),
                    CVPixelBufferGetHeight(surface).into(),
                );
                let new_bounds = self.object_fit.get_bounds(bounds, size);
                let mut style = Style::default();
                style.refine(&self.style);
                let corner_radii = style.corner_radii.to_pixels(window.rem_size());
                window.with_element_opacity(style.opacity, |window| {
                    window.paint_surface_with_corner_radii(
                        new_bounds,
                        bounds,
                        corner_radii,
                        surface.clone(),
                    );
                });
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

impl Styled for Surface {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod tests {
    use super::*;
    use crate::{Context, Render, TestAppContext, div, prelude::*, px};
    use std::ptr;

    struct SurfaceView {
        source: CFRetained<CVPixelBuffer>,
        appearance: Option<(f32, Pixels)>,
    }

    impl Render for SurfaceView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let child = surface(self.source.clone()).w(px(32.)).h(px(16.));
            let child = if let Some((opacity, radius)) = self.appearance {
                child.opacity(opacity).rounded(radius)
            } else {
                child
            };
            div().opacity(0.5).child(child)
        }
    }

    #[crate::test]
    fn surface_style_paints_inherited_opacity_and_scaled_frame_corners(cx: &mut TestAppContext) {
        use objc2_core_video::{CVPixelBufferCreate, kCVPixelFormatType_32BGRA};
        let mut raw = ptr::null_mut();
        // SAFETY: CoreVideo initializes the live out pointer with an owned buffer.
        assert_eq!(
            unsafe {
                CVPixelBufferCreate(
                    None,
                    16,
                    16,
                    kCVPixelFormatType_32BGRA,
                    None,
                    ptr::NonNull::from(&mut raw),
                )
            },
            0
        );
        // SAFETY: A successful create call transfers one non-null retained reference.
        let source = unsafe { CFRetained::from_raw(ptr::NonNull::new(raw).expect("pixel buffer")) };
        let window = cx.add_window(|_, _| SurfaceView {
            source,
            appearance: None,
        });
        for scale in [1., 2.] {
            cx.simulate_window_scale_factor_change(window.into(), scale);
            for (appearance, expected_opacity, radius) in [
                (None, 0.5, 0.),
                (Some((0.5, px(3.25))), 0.25, 3.25),
                (Some((0., px(3.25))), 0., 3.25),
            ] {
                window
                    .update(cx, |view, _, cx| {
                        view.appearance = appearance;
                        cx.notify();
                    })
                    .unwrap();
                cx.update_window(window.into(), |_, window, cx| {
                    window.draw(cx).clear(cx);
                    let surfaces = &window.rendered_scene().surfaces;
                    assert_eq!(surfaces.len(), 1);
                    let surface = &surfaces[0];
                    assert_eq!(surface.opacity, expected_opacity);
                    assert_eq!(
                        surface.corner_radii,
                        crate::Corners::all(crate::ScaledPixels(radius * scale))
                    );
                    assert_eq!(
                        surface.corner_bounds.size.width,
                        crate::ScaledPixels(32. * scale)
                    );
                    assert_eq!(surface.bounds.size.width, crate::ScaledPixels(16. * scale));
                })
                .unwrap();
            }
        }
    }
}
