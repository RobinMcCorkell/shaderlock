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

const vec3 LUMIN = vec3(0.299, 0.587, 0.114);
const vec4 CLEAR_COLOR = vec4(0.0, 0.0, 0.0, 1.0);

const float DESATURATE_SPEED = 0.2;
const float DESATURATE_ACCEL = 2.2;
const float FADE_SPEED = 0.2;
const float FADE_ACCEL = 1.0;

vec4 desaturate(vec4 color, float desaturation) {
    vec3 gray = vec3(dot(LUMIN, vec3(color)));
    return vec4(mix(vec3(color), gray, desaturation), 1.0);
}


void main() {
    float desaturation = clamp(pow(DESATURATE_SPEED * iTime, DESATURATE_ACCEL), 0.0, 1.0);
    float fade = clamp(pow(FADE_SPEED * (iTime - 1.0/DESATURATE_SPEED), FADE_ACCEL), 0.0, 1.0);

    vec4 ouv = iTransform * vec4(gl_FragCoord.xy, 0.0, 1.0);
    vec2 uv = ouv.xy / ouv.w;

    vec4 desaturated = desaturate(texture(sampler2D(t_screenshot, s_screenshot), uv), desaturation);
    f_color = mix(desaturated, CLEAR_COLOR, fade);
    f_color = mix(f_color, vec4(0.0, 0.0, 0.0, 1.0), iFadeAmount);
}
