// Tests extracted from crates/clay-ui/src/layout/media.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn image_fit_computes_contain_and_cover_rects() {
    let container = Rect::new(0.0, 0.0, 200.0, 100.0);
    let natural = Size::new(50.0, 50.0);

    assert_eq!(
        UiImageFit::Contain.fitted_rect(container, natural, glam::Vec2::splat(0.5)),
        Rect::new(50.0, 0.0, 100.0, 100.0)
    );
    assert_eq!(
        UiImageFit::Cover.fitted_rect(container, natural, glam::Vec2::splat(0.5)),
        Rect::new(0.0, -50.0, 200.0, 200.0)
    );
}

#[test]
fn raster_image_validates_rgba_length() {
    let image = UiRasterImage::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap();

    assert_eq!(image.size(), Size::new(2.0, 1.0));
    assert_eq!(image.byte_len(), 8);
    assert!(matches!(
        UiRasterImage::from_rgba8(2, 1, vec![0, 0, 0]).unwrap_err(),
        UiImageError::InvalidPixelLength {
            expected: 8,
            actual: 3
        }
    ));
}

#[test]
fn decodes_png_to_rgba8() {
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &[255, 0, 0, 255],
        1,
        1,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();

    let image = UiRasterImage::decode(&png).unwrap();

    assert_eq!(image.width, 1);
    assert_eq!(image.height, 1);
    assert_eq!(image.pixels.len(), 4);
    assert_eq!(image.pixels[3], 255);
}

#[test]
fn svg_rasterization_exposes_supersampling_dials() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8">
        <circle cx="4" cy="4" r="3" fill="white"/>
    </svg>"#;
    let document = SvgDocument::parse(svg).unwrap();
    let raster = document
        .rasterize(
            SvgRasterOptions::default()
                .target_size(Size::new(16.0, 16.0))
                .antialiasing(UiAntialiasing::supersampled(3)),
        )
        .unwrap();

    assert_eq!(document.size(), Size::new(8.0, 8.0));
    assert_eq!(raster.size(), Size::new(16.0, 16.0));
    assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
}

#[test]
fn svg_rasterization_rejects_oversized_supersampling() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
        <rect width="100" height="100" fill="white"/>
    </svg>"#;
    let document = SvgDocument::parse(svg).unwrap();

    let error = document
        .rasterize(
            SvgRasterOptions::default()
                .antialiasing(UiAntialiasing::supersampled(4))
                .max_output_pixels(100),
        )
        .unwrap_err();

    assert!(matches!(error, UiImageError::RenderTargetTooLarge { .. }));
}
