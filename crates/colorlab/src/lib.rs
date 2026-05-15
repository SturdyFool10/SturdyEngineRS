#![allow(dead_code)]

pub mod colorspaces;

pub use colorspaces::adobe_rgb::AdobeRgb;
pub use colorspaces::color::Color;
pub use colorspaces::colorspace::ColorSpace;
pub use colorspaces::display_p3::DisplayP3;
pub use colorspaces::hsl::Hsl;
pub use colorspaces::hsv::Hsv;
pub use colorspaces::hwb::Hwb;
pub use colorspaces::interpolate::Interpolate;
pub use colorspaces::lab::Lab;
pub use colorspaces::lch::Lch;
pub use colorspaces::luv::Luv;
pub use colorspaces::oklab::Oklab;
pub use colorspaces::oklch::Oklch;
pub use colorspaces::rec2020::Rec2020;
pub use colorspaces::srgb::Srgb;
pub use colorspaces::xyz::Xyz;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64, epsilon: f64) {
        assert!(
            (actual - expected).abs() <= epsilon,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_color_close(actual: Color, expected: Color, epsilon: f64) {
        assert_close(actual.r, expected.r, epsilon);
        assert_close(actual.g, expected.g, epsilon);
        assert_close(actual.b, expected.b, epsilon);
        assert_close(actual.a, expected.a, epsilon);
    }

    fn sample_color() -> Color {
        Color::new(0.25, 0.5, 0.75, 0.6)
    }

    #[test]
    fn srgb_round_trip_preserves_linear_color() {
        let original = sample_color();
        let encoded = Srgb::from_color(&original);
        let decoded = encoded.to_color();
        assert_color_close(decoded, original, 1e-4);
    }

    #[test]
    fn cylindrical_spaces_round_trip_preserves_linear_color() {
        let original = sample_color();

        let hsl = Hsl::from_color(&original);
        assert_color_close(hsl.to_color(), original, 1e-4);

        let hsv = Hsv::from_color(&original);
        assert_color_close(hsv.to_color(), original, 1e-4);

        let hwb = Hwb::from_color(&original);
        assert_color_close(hwb.to_color(), original, 1e-4);
    }

    #[test]
    fn lab_round_trip_stays_within_expected_tolerance() {
        let original = sample_color();
        let lab = Lab::from_color(&original);
        assert_color_close(lab.to_color(), original, 5e-3);
    }

    #[test]
    fn lch_round_trip_stays_within_expected_tolerance() {
        let original = sample_color();
        let lch = Lch::from_color(&original);
        assert_color_close(lch.to_color(), original, 5e-3);
    }

    #[test]
    fn oklab_round_trip_stays_within_expected_tolerance() {
        let original = sample_color();
        let oklab = Oklab::from_color(&original);
        assert_color_close(oklab.to_color(), original, 5e-3);
    }

    #[test]
    fn oklch_round_trip_stays_within_expected_tolerance() {
        let original = sample_color();
        let oklch = Oklch::from_color(&original);
        assert_color_close(oklch.to_color(), original, 5e-3);
    }

    #[test]
    fn xyz_round_trip_preserves_linear_color() {
        let original = sample_color();
        let xyz = Xyz::from_color(&original);
        let round_trip = xyz.to_color();
        assert_color_close(round_trip, original, 1e-4);
    }

    #[test]
    fn wide_gamut_spaces_preserve_alpha_through_round_trip() {
        let original = sample_color();

        let adobe = AdobeRgb::from_color(&original).to_color();
        assert_close(adobe.a, original.a, 1e-4);

        let p3 = DisplayP3::from_color(&original).to_color();
        assert_close(p3.a, original.a, 1e-4);

        let rec2020 = Rec2020::from_color(&original).to_color();
        assert_close(rec2020.a, original.a, 1e-4);
    }

    #[test]
    fn from_srgb_u8_decodes_gamma_to_linear() {
        // sRGB(255, 255, 255) -> linear white
        let white = Color::from_srgb_u8(255, 255, 255);
        assert_close(white.r, 1.0, 1e-6);
        assert_close(white.a, 1.0, 1e-6);

        // sRGB(0, 0, 0) -> linear black
        let black = Color::from_srgb_u8(0, 0, 0);
        assert_close(black.r, 0.0, 1e-6);

        // sRGB 128 is not linear 0.5
        let mid = Color::from_srgb_u8(128, 128, 128);
        assert!(
            mid.r < 0.22 && mid.r > 0.21,
            "expected ~0.216, got {}",
            mid.r
        );
    }

    #[test]
    fn from_srgba_u8_passes_alpha_through_unchanged() {
        let c = Color::from_srgba_u8(255, 0, 0, 128);
        assert_close(c.a, 128.0 / 255.0, 1e-6);
        assert_close(c.r, 1.0, 1e-6);
    }

    #[test]
    fn to_srgb_u8_round_trips_primary_colors() {
        for &(r, g, b) in &[
            (255u8, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (0, 0, 0),
            (255, 255, 255),
        ] {
            let c = Color::from_srgb_u8(r, g, b);
            let [ro, go, bo, ao] = c.to_srgb_u8();
            assert_eq!((ro, go, bo, ao), (r, g, b, 255));
        }
    }

    #[test]
    fn from_hex_parses_all_formats() {
        // #RGB
        let c = Color::from_hex("#f00").unwrap();
        let [r, g, b, _] = c.to_srgb_u8();
        assert_eq!((r, g, b), (255, 0, 0));

        // #RGBA
        let c = Color::from_hex("#f00f").unwrap();
        let [r, g, b, a] = c.to_srgb_u8();
        assert_eq!((r, g, b, a), (255, 0, 0, 255));

        // #RRGGBB
        let c = Color::from_hex("#ff8000").unwrap();
        let [r, g, b, _] = c.to_srgb_u8();
        assert_eq!((r, g, b), (255, 128, 0));

        // #RRGGBBAA
        let c = Color::from_hex("#ff800080").unwrap();
        let [r, g, b, a] = c.to_srgb_u8();
        assert_eq!((r, g, b, a), (255, 128, 0, 128));

        // without leading #
        let c = Color::from_hex("ff0000").unwrap();
        let [r, g, b, _] = c.to_srgb_u8();
        assert_eq!((r, g, b), (255, 0, 0));

        // uppercase
        let c = Color::from_hex("#FF0000").unwrap();
        let [r, g, b, _] = c.to_srgb_u8();
        assert_eq!((r, g, b), (255, 0, 0));
    }

    #[test]
    fn from_hex_rejects_invalid_input() {
        assert!(Color::from_hex("#gg0000").is_err());
        assert!(Color::from_hex("#12345").is_err());
        assert!(Color::from_hex("").is_err());
    }

    #[test]
    fn to_hex_string_omits_alpha_when_opaque() {
        assert_eq!(
            Color::from_hex("#ff8000").unwrap().to_hex_string(),
            "#ff8000"
        );
        let semi = Color::from_srgba_u8(255, 128, 0, 128);
        let hex = semi.to_hex_string();
        assert_eq!(hex.len(), 9); // #rrggbbaa
        assert!(hex.starts_with('#'));
    }

    #[test]
    fn blend_in_linear_midpoint_is_average() {
        let red = Color::RED;
        let blue = Color::BLUE;
        let mid = red.blend_in::<Color>(&blue, 0.5);
        assert_close(mid.r, 0.5, 1e-9);
        assert_close(mid.g, 0.0, 1e-9);
        assert_close(mid.b, 0.5, 1e-9);
    }

    #[test]
    fn blend_in_oklch_stays_saturated_through_midpoint() {
        let red = Color::from_hex("#ff0000").unwrap();
        let blue = Color::from_hex("#0000ff").unwrap();

        let linear_mid = red.blend_in::<Color>(&blue, 0.5);
        let oklch_mid = red.blend_in::<Oklch>(&blue, 0.5);

        // Oklch midpoint should be more saturated (higher chroma) than linear
        let linear_oklab = Oklab::from_color(&linear_mid);
        let oklch_oklab = Oklab::from_color(&oklch_mid);
        let linear_chroma =
            (linear_oklab.a * linear_oklab.a + linear_oklab.b * linear_oklab.b).sqrt();
        let oklch_chroma = (oklch_oklab.a * oklch_oklab.a + oklch_oklab.b * oklch_oklab.b).sqrt();
        assert!(
            oklch_chroma > linear_chroma,
            "Oklch blend should preserve chroma better than linear"
        );
    }

    #[test]
    fn blend_in_hsl_wraps_hue_shortest_path() {
        // Red (hue ~0) blended with a color at hue 300 should go through ~330, not ~150
        let red = Color::RED;
        let pink = Color::from_hex("#ff00ff").unwrap(); // magenta, hue ~300
        let mid = red.blend_in::<Hsl>(&pink, 0.5);
        let hsl_mid = Hsl::from_color(&mid);
        // Shortest path: 0 and 300 -> diff is -60 -> midpoint is 330 (or 0+(-30)=330)
        assert!(
            hsl_mid.h > 270.0 || hsl_mid.h < 30.0,
            "hue should wrap, got {}",
            hsl_mid.h
        );
    }

    #[test]
    fn blend_in_endpoints_return_original_colors() {
        let a = sample_color();
        let b = Color::from_srgb_u8(200, 100, 50);
        // Each endpoint goes through the color space's round-trip, so use its tolerance (Oklab: 5e-3).
        assert_color_close(a.blend_in::<Oklab>(&b, 0.0), a, 5e-3);
        assert_color_close(a.blend_in::<Oklab>(&b, 1.0), b, 5e-3);
    }
}
