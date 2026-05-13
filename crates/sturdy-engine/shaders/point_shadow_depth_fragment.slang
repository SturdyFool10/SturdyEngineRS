struct V {
    float4 pos : SV_POSITION;
    float d : TEXCOORD0;
};

float main(V v) : SV_DEPTH {
    return v.d;
}
