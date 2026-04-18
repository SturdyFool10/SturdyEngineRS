// Triangle fragment shader - outputs a solid gray color
// Used for fullscreen triangle passes that don't require vertex shader output

float4 ps_main() : SV_TARGET { return float4(0.5, 0.5, 0.5, 1.0); }
