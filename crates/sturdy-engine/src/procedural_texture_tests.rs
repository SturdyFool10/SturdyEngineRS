// Tests extracted from crates/sturdy-engine/src/procedural_texture.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn checker_recipe_alternates_tiles() {
    let recipe = ProceduralTextureRecipe::Checker {
        tile_size: 2,
        color_a: [1, 2, 3, 4],
        color_b: [5, 6, 7, 8],
    };
    assert_eq!(recipe.sample(0, 0, 8, 8, 0), [1, 2, 3, 4]);
    assert_eq!(recipe.sample(2, 0, 8, 8, 0), [5, 6, 7, 8]);
    assert_eq!(recipe.sample(2, 2, 8, 8, 0), [1, 2, 3, 4]);
}

#[test]
fn horizontal_ramp_reaches_endpoints() {
    let recipe = ProceduralTextureRecipe::HorizontalRamp {
        left: [0, 10, 20, 30],
        right: [100, 110, 120, 130],
    };
    assert_eq!(recipe.sample(0, 0, 3, 1, 0), [0, 10, 20, 30]);
    assert_eq!(recipe.sample(2, 0, 3, 1, 0), [100, 110, 120, 130]);
}
