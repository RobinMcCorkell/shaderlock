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

vec4 Desaturate(vec3 color, float Desaturation) {
    vec3 grayXfer = vec3(0.3, 0.59, 0.11);
    vec3 gray = vec3(dot(grayXfer, color));
    return vec4(mix(color, gray, Desaturation), 1.0);
}

const float desaturateDuration = 5.0;
const float desaturateFactor = 2.2;
const float fadeDuration = 5.0;
const float fadeFactor = 1.0;

void main() {
    float desaturate = clamp(pow(iTime / desaturateDuration, desaturateFactor), 0.0, 1.0);
    float fade = clamp(pow((iTime - desaturateDuration) / fadeDuration, fadeFactor), 0.0, 1.0);

    vec4 ouv = iTransform * vec4(gl_FragCoord.xy, 0.0, 1.0);
    vec2 uv = ouv.xy / ouv.w;

    f_color = mix(Desaturate(texture(sampler2D(t_screenshot, s_screenshot), uv).rgb, desaturate), vec4(0.0, 0.0, 0.0, 1.0), fade);
}
