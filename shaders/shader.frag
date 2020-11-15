#version 450

layout(location=0) out vec4 f_color;

layout(set = 0, binding = 0) uniform texture2D t_screenshot;
layout(set = 0, binding = 1) uniform sampler s_screenshot;
layout(set = 0, binding = 2) uniform Uniforms {
    vec2 iResolution;
};

// layout(set = 1, binding = 0) uniform FrameUniforms {
//     float iTime;
// };

void main() {
    vec2 tex_coord = gl_FragCoord.xy / iResolution;
    tex_coord.y = 1.0 - tex_coord.y;
    f_color = texture(sampler2D(t_screenshot, s_screenshot), tex_coord);
}
