// OIT fragment collection pass (Per-Pixel Linked List).
//
// Renders all Translucent-domain objects and inserts each fragment into a
// per-pixel linked list. The linked list is stored in three GPU buffers:
//
//   oit_head_buf   — RWStructuredBuffer<uint>, one entry per pixel (row-major),
//                    initialized to OIT_INVALID before this pass.
//   oit_frag_pool  — RWStructuredBuffer<OitFragment>, global fragment pool.
//   oit_counter    — RWStructuredBuffer<uint>, single atomic counter.
//
// Fragment insertion is lock-free: atomic exchange on the head pointer + atomic
// increment on the counter. Fragments at a given pixel form a singly linked list
// in insertion order; oit_resolve.slang sorts them by depth before blending.
//
// When the pool is full (counter >= pool_size), fragments are silently discarded
// rather than corrupting memory. Configure OitConfig::average_layers to match
// your scene's transparency density.
//
// Roadmap: Track 6d — OIT translucent forward tail.

static const uint OIT_INVALID = 0xFFFFFFFFu;

struct OitFragment {
    /// Linear premultiplied RGBA in f16 (xyz = color × alpha, w = alpha).
    /// Premultiplied avoids an extra multiply at blend time.
    uint  color_rg;  // f16 R in low bits, f16 G in high bits
    uint  color_ba;  // f16 B in low bits, f16 A in high bits
    float depth;     // NDC depth (SV_Position.z)
    uint  next;      // index of next fragment, OIT_INVALID = end of list
};

RWStructuredBuffer<uint>        oit_head_buf;   // [pixel_y * push.fb_width + pixel_x]
RWStructuredBuffer<OitFragment> oit_frag_pool;  // [0 .. pool_size)
RWStructuredBuffer<uint>        oit_counter;    // [0] = next free slot

struct MaterialConstants {
    float4 albedo;
    float4 emissive_metallic;
    float4 roughness_flags;
};
StructuredBuffer<MaterialConstants> material_desc;

struct OitPushConstants {
    float4x4 view_proj;
    float4x4 previous_view_proj;
    float    time;
    float3   _pad_cam;
    uint     fb_width;      // framebuffer pixel width (for head_buf indexing)
    uint     pool_size;     // total capacity of oit_frag_pool
    float2   _pad_oit;
};
uniform OitPushConstants push;

struct VSOut {
    float4 position : SV_POSITION;
    float3 normal   : NORMAL;
    float2 uv       : TEXCOORD0;
    float3 world_pos: TEXCOORD1;
    float4 tangent  : TANGENT;
};

// ── Helpers ───────────────────────────────────────────────────────────────────

uint pack_f16x2(float a, float b) {
    return (uint(f32tof16(a)) & 0xFFFFu) | (uint(f32tof16(b)) << 16u);
}

// ── Main ─────────────────────────────────────────────────────────────────────

void main(VSOut v) {
    // Read material constants.
    MaterialConstants M = material_desc[0];
    float3 base_color = M.albedo.rgb;
    float  alpha      = M.albedo.a;
    float3 emissive   = M.emissive_metallic.xyz;

    // Combine base colour and emissive. Opacity from the material alpha.
    // Full forward-lit PBR (Track 6b ForwardLitVariant) will replace this
    // simple accumulation once the material variant compiler is wired in.
    float3 lit_color = base_color + emissive;

    // Skip nearly-invisible fragments early.
    if (alpha < 1.0 / 255.0) return;

    // Allocate a slot in the fragment pool (atomic).
    uint slot;
    InterlockedAdd(oit_counter[0], 1u, slot);
    if (slot >= push.pool_size) return; // pool full — discard gracefully

    // Pack premultiplied color (color × alpha) + alpha into f16 pairs.
    float3 premul = lit_color * alpha;
    OitFragment frag;
    frag.color_rg = pack_f16x2(premul.r, premul.g);
    frag.color_ba = pack_f16x2(premul.b, alpha);
    frag.depth    = v.position.z;
    frag.next     = OIT_INVALID;
    oit_frag_pool[slot] = frag;

    // Link into the per-pixel list (atomic exchange).
    uint px = uint(v.position.x);
    uint py = uint(v.position.y);
    uint head_idx = py * push.fb_width + px;
    uint old_head;
    InterlockedExchange(oit_head_buf[head_idx], slot, old_head);
    oit_frag_pool[slot].next = old_head; // chain old list behind the new fragment
}
