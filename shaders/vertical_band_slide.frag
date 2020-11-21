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

const vec4 CLEAR_COLOR = vec4(0.0, 0.0, 0.0, 1.0);

const int BAND_SIZE = 64;
const int BORDER = 1;
const float SPEED = 0.8;
const float ACCEL = 5.0;

void main() {
    vec2 offset;
    vec4 coord = gl_FragCoord;
    int double_band_coord = int(mod(coord.x, 2*BAND_SIZE));
    int band_coord = int(mod(double_band_coord, BAND_SIZE));
    if (band_coord < BORDER) {
        f_color = CLEAR_COLOR;
        return;
    } else if (double_band_coord < BAND_SIZE) {
        offset = vec2(0.0, 1.0);
    } else {
        offset = vec2(0.0, -1.0);
    }
    coord.xy += offset * pow(SPEED * iTime, ACCEL);

    vec4 ouv = iTransform * coord;
    vec2 uv = ouv.xy / ouv.w;

    if (uv.x > 1.0 || uv.x < 0.0 || uv.y > 1.0 || uv.y < 0.0) {
        f_color = CLEAR_COLOR;
    } else {
        f_color = texture(sampler2D(t_screenshot, s_screenshot), uv);
    }
}
