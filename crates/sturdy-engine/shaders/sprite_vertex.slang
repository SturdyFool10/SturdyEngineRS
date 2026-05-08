// Sprite vertex shader — pixel-space to NDC transform.
//
// Accepts vertices in pixel coordinates and transforms to NDC using the
// viewport push constants. Rotation and scale are pre-baked into vertex
// positions by SpriteBatch on the CPU.
//
// Roadmap: 2D and instanced rendering — first-class 2D sprite/batch path.

struct SpriteConstants {
    // Viewport size in pixels (for pixel → NDC conversion).
    // NDC x = pixel_x / (viewport_w * 0.5) - 1
    // NDC y = 1 - pixel_y / (viewport_h * 0.5)
    float viewport_w;
    float viewport_h;
    float _pad0;
    float _pad1;
};
uniform SpriteConstants push;

struct VSInput {
    float2 position : POSITION;  // pixel coordinates (top-left origin, Y-down)
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

VSOut main(VSInput input) {
    // Convert from pixel coordinates (Y-down, origin top-left) to NDC (Y-up, origin center).
    float ndcx =  input.position.x / (push.viewport_w * 0.5) - 1.0;
    float ndcy = -(input.position.y / (push.viewport_h * 0.5) - 1.0);
    VSOut o;
    o.position = float4(ndcx, ndcy, 0.0, 1.0);
    o.uv       = input.uv;
    o.color    = input.color;
    return o;
}
