#version 450

layout(location=0) out vec4 f_color;

layout(set = 0, binding = 0) uniform texture2D t_screenshot;
layout(set = 0, binding = 1) uniform sampler s_screenshot;
layout(set = 0, binding = 2) uniform Uniforms {
    mat4 iTransform;
};

layout(push_constant) uniform FrameUniforms {
    float iTime;
};

const float PI = 3.141529;
const float SPEED = 0.3;
const float DIRECTIONS = 4.0;
const float LOD_BIAS = 3.0;

vec4 overlay(vec2 uv, float amount) {
    vec4 color = vec4(0.0);
    for (float d = 0.0; d < PI; d += PI/DIRECTIONS) {
        color += texture(sampler2D(t_screenshot, s_screenshot), uv + vec2(cos(d), sin(d)) * amount, LOD_BIAS);
    }
    return color / DIRECTIONS;
}

void main() {
    float amount = SPEED * iTime;

    vec4 ouv = iTransform * vec4(gl_FragCoord.xy, 0.0, 1.0);
    vec2 uv = ouv.xy / ouv.w;

    f_color = overlay(uv, amount);
}
