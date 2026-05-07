// OIT resolve pass (Per-Pixel Linked List).
//
// Traverses the per-pixel linked list built by oit_collect.slang, sorts up to
// MAX_OIT_LAYERS fragments by depth (front-to-back), then composites them over
// the existing opaque HDR colour using standard "src-over" alpha blending.
//
// After compositing, the head buffer and counter are reset so they are ready
// for the next frame. (Reset is done in DeferredPass on CPU before the
// collect pass, not here, to avoid a dependency on the resolve completing
// before next frame's collect begins.)

static const uint OIT_INVALID   = 0xFFFFFFFFu;
static const uint MAX_OIT_LAYERS = 16u;  // max sorted layers per pixel

struct OitFragment {
    uint  color_rg;
    uint  color_ba;
    float depth;
    uint  next;
};

Texture2D<float4>               opaque_hdr;     // "scene_color" from the deferred pass
SamplerState                    opaque_sampler;
StructuredBuffer<uint>          oit_head_buf;
StructuredBuffer<OitFragment>   oit_frag_pool;

struct ResolvePushConstants {
    uint fb_width;
    float3 _pad;
};
uniform ResolvePushConstants push;

struct VSOut { float4 position : SV_POSITION; float2 uv : TEXCOORD0; };

// ── Helpers ───────────────────────────────────────────────────────────────────

float4 unpack_fragment(OitFragment f) {
    float r = f16tof32(f.color_rg & 0xFFFFu);
    float g = f16tof32(f.color_rg >> 16u);
    float b = f16tof32(f.color_ba & 0xFFFFu);
    float a = f16tof32(f.color_ba >> 16u);
    return float4(r, g, b, a); // premultiplied rgb + alpha
}

// Insertion sort (optimal for small N, branchless for GPU).
void sort_by_depth(inout OitFragment frags[MAX_OIT_LAYERS], uint count) {
    for (uint i = 1; i < count; ++i) {
        OitFragment key = frags[i];
        int j = int(i) - 1;
        while (j >= 0 && frags[j].depth > key.depth) {
            frags[j + 1] = frags[j];
            --j;
        }
        frags[j + 1] = key;
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

float4 main(VSOut v) : SV_TARGET {
    uint px = uint(v.position.x);
    uint py = uint(v.position.y);
    uint head_idx = py * push.fb_width + px;

    uint head = oit_head_buf[head_idx];
    if (head == OIT_INVALID) {
        // No transparent fragments — return opaque unchanged.
        return opaque_hdr.SampleLevel(opaque_sampler, v.uv, 0);
    }

    // Collect fragments into a local array.
    OitFragment frags[MAX_OIT_LAYERS];
    uint count = 0;
    uint cur = head;
    while (cur != OIT_INVALID && count < MAX_OIT_LAYERS) {
        frags[count++] = oit_frag_pool[cur];
        cur = oit_frag_pool[cur].next;
    }

    // Sort front-to-back by depth (smallest NDC depth = closest to camera).
    sort_by_depth(frags, count);

    // Composite transparent layers from back to front (Porter-Duff src-over).
    // We iterate back-to-front so we accumulate behind the opaque background.
    float4 opaque = opaque_hdr.SampleLevel(opaque_sampler, v.uv, 0);
    float3 accum_rgb = opaque.rgb;
    float  transmittance = 1.0; // 1 = nothing blocked yet

    // Walk front-to-back, building transmittance.
    // Equivalent to back-to-front src-over when done in one pass:
    //   result = lerp(result, src.rgb, src.a)
    // We accumulate: T(i) = product of (1-alpha) for i<j
    // Final: sum_j(color_j * alpha_j * T(j)) + T(total) * opaque
    float3 transparent_accum = float3(0.0, 0.0, 0.0);
    float  total_transmittance = 1.0;

    for (uint i = 0; i < count; ++i) {
        float4 c = unpack_fragment(frags[i]); // premultiplied rgb, alpha
        transparent_accum += c.rgb * total_transmittance; // pre-mul so no extra ×alpha
        total_transmittance *= (1.0 - c.a);
    }

    float3 final_color = transparent_accum + total_transmittance * opaque.rgb;
    return float4(final_color, 1.0);
}
