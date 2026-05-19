// Tests extracted from crates/sturdy-engine-core/src/sampler.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn sampler_desc_rejects_non_finite_custom_border_color() {
    let desc = SamplerDesc {
        border_color: BorderColor::Custom([0.0, f32::NAN, 0.0, 1.0]),
        ..SamplerDesc::default()
    };

    assert!(matches!(desc.validate(), Err(Error::InvalidInput(_))));
}
