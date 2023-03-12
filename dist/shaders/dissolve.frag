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

const float DISSOLVE_SPEED = 200;
const float FALL_ACCEL = 800.0;
const float RANDOM_AMOUNT = 0.15;
const float RANDOM_SIZE = 60.0;

const vec4 CLEAR_COLOR = vec4(0.0, 0.0, 0.0, 1.0);

// --------------------------------------------------------------
// hash() and noise() are from https://www.shadertoy.com/view/Msf3WH:

// The MIT License
// Copyright © 2013 Inigo Quilez
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions: The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
// https://www.youtube.com/c/InigoQuilez
// https://iquilezles.org
vec2 hash( vec2 p ) {
	p = vec2( dot(p,vec2(127.1,311.7)), dot(p,vec2(269.5,183.3)) );
	return -1.0 + 2.0*fract(sin(p)*43758.5453123);
}

float noise( in vec2 p ) {
    const float K1 = 0.366025404; // (sqrt(3)-1)/2;
    const float K2 = 0.211324865; // (3-sqrt(3))/6;

	vec2  i = floor( p + (p.x+p.y)*K1 );
    vec2  a = p - i + (i.x+i.y)*K2;
    float m = step(a.y,a.x);
    vec2  o = vec2(m,1.0-m);
    vec2  b = a - o + K2;
	vec2  c = a - 1.0 + 2.0*K2;
    vec3  h = max( 0.5-vec3(dot(a,a), dot(b,b), dot(c,c) ), 0.0 );
	vec3  n = h*h*h*h*vec3( dot(a,hash(i+0.0)), dot(b,hash(i+o)), dot(c,hash(i+1.0)));
    return dot( n, vec3(70.0) );
}
// --------------------------------------------------------------

float dissolve(float y, float time) {
    float t = max(0.0, time - y / DISSOLVE_SPEED);
    // s = 1/2 a (t - s/v)^2
    float dissolve_t = (
        t
        + DISSOLVE_SPEED / FALL_ACCEL
        - sqrt(DISSOLVE_SPEED * DISSOLVE_SPEED + 2.0 * t * DISSOLVE_SPEED * FALL_ACCEL) / FALL_ACCEL
    );
    float dissolve_y = DISSOLVE_SPEED * dissolve_t;
    return y + dissolve_y;
}

bool is_closer_rounded(float x, float other) {
    float x_round = round(x);
    float x_err = abs(x - x_round);

    float other_round = round(other);
    float other_err = abs(other - other_round);

    bool is_same = abs(other_round - x_round) < 0.1;
    return is_same && other_err < x_err;
}

void main() {
    float rand = RANDOM_AMOUNT * noise(vec2(gl_FragCoord.x/RANDOM_SIZE, 0.0));
    float t = iTime * (1.0 + rand);
    float y = dissolve(gl_FragCoord.y, t);

    vec4 ouv = iTransform * vec4(gl_FragCoord.x, round(y), 0.0, 1.0);
    vec2 uv = ouv.xy / ouv.w;

    float prev_y = dissolve(gl_FragCoord.y - 1, t);
    float next_y = dissolve(gl_FragCoord.y + 1, t);

    bool is_blank = is_closer_rounded(y, prev_y) || is_closer_rounded(y, next_y);

    if (is_blank || uv.y > 1.0) {
        f_color = CLEAR_COLOR;
    } else {
        f_color = texture(sampler2D(t_screenshot, s_screenshot), uv);
    }
    f_color = mix(f_color, vec4(0.0, 0.0, 0.0, 1.0), iFadeAmount);
}
