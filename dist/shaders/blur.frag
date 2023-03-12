#version 450

layout(location=0) out vec4 f_color;

layout(set = 0, binding = 0) uniform texture2D t_screenshot;
layout(set = 0, binding = 1) uniform sampler s_screenshot;
layout(set = 0, binding = 2) uniform Uniforms {
    mat4 iTransform;
};

layout(push_constant) uniform FrameUniforms {
    float iTime;
    float iFadeAmount;
};

const float AMOUNT = 4.0;
const float SPEED = 0.1;
const float ACCEL = 1.5;

void main() {
    float lod_bias = AMOUNT * clamp(pow(SPEED * iTime, ACCEL), 0.0, 1.0);

    vec4 ouv = iTransform * vec4(gl_FragCoord.xy, 0.0, 1.0);
    vec2 uv = ouv.xy / ouv.w;

    f_color = texture(sampler2D(t_screenshot, s_screenshot), uv, lod_bias);
    f_color = mix(f_color, vec4(0.0, 0.0, 0.0, 1.0), iFadeAmount);
}
