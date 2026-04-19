// Passthrough fragment shader.
// Samples the frame image named "source" and writes it unchanged.
// Used by ShaderProgram::passthrough() and GraphImage::blit_from().

Texture2D<float4> source;
SamplerState source_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

float4 main(VSOut input) : SV_TARGET {
    return source.SampleLevel(source_sampler, input.uv, 0.0);
}
