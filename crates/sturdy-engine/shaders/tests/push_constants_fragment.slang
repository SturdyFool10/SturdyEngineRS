struct Pc {
    float brightness;
};

float4 main(uniform Pc pc) : SV_TARGET {
    return float4(pc.brightness, 0.0, 0.0, 1.0);
}
