#version 450

const vec2 positions[4] = vec2[4](
    vec2(-1.0, -1.0),
    vec2(-1.0, 1.0),
    vec2(1.0, -1.0),
    vec2(1.0, 1.0)
);

const vec2 tex_positions[4] = vec2[4](
    vec2(0.0, 1.0),
    vec2(0.0, 0.0),
    vec2(1.0, 1.0),
    vec2(1.0, 0.0)
);

layout(location=0) out vec2 v_tex_coords;

layout(set = 0, binding = 2) uniform Uniforms {
    mat4 iTransform;
};

void main() {
    gl_Position = iTransform * vec4(positions[gl_VertexIndex], 0.0, 1.0);
    v_tex_coords = tex_positions[gl_VertexIndex];
}
