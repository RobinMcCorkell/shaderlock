#version 450

layout(location=0) out vec4 f_color;

layout(set = 0, binding = 0) uniform texture2D t_screenshot;
layout(set = 0, binding = 1) uniform sampler s_screenshot;
layout(set = 0, binding = 2) uniform Uniforms {
    vec2 iResolution;
};

layout(push_constant) uniform FrameUniforms {
    float iTime;
};

vec4 Desaturate(vec3 color, float Desaturation) {
    vec3 grayXfer = vec3(0.3, 0.59, 0.11);
    vec3 gray = vec3(dot(grayXfer, color));
    return vec4(mix(color, gray, Desaturation), 1.0);
}

void main() {
    vec2 uv = gl_FragCoord.xy / iResolution;
    uv.y = 1.0 - uv.y;
    float desaturate = min(pow(iTime / 10.0, 1.0), 1.0);
    float fade = min(pow(iTime / 10.0, 5.0), 1.0);
    f_color = mix(Desaturate(texture(sampler2D(t_screenshot, s_screenshot), uv).rgb, desaturate), vec4(0.0, 0.0, 0.0, 1.0), fade);
}
