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

const float NOISE_FREQ = 12.0;
const float SPEED = 0.25;
const float DISTANCE_FACTOR = 3.0;
const vec2 START_POINT = vec2(1.0, 1.0);

const vec4 SCORCH_COLOR = vec4(0.0, 0.0, 0.0, 1.0);
const float SCORCH_BAND_SIZE = 0.4;
const float SCORCH_MAX = 0.8;

const vec4 BURN_COLOR = vec4(0.93, 0.35, 0.02, 1.0);
const float BURN_BAND_SIZE = 0.1;
const float BURN_MAX = 0.5;


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

void main() {
    vec4 ouv = iTransform * vec4(gl_FragCoord.xy, 0.0, 1.0);
    vec2 uv = ouv.xy / ouv.w;

    float d = distance(uv, START_POINT);

	float v = noise(NOISE_FREQ * uv);
	v = 0.5 + 0.5*v;
    v += d*DISTANCE_FACTOR;

    float f = (SPEED * iTime) - v;

    float scorch = smoothstep(0.0, 1.0, f/SCORCH_BAND_SIZE);
    float burn = smoothstep(0.0, 1.0, (f-SCORCH_BAND_SIZE)/BURN_BAND_SIZE);
    float alpha = smoothstep(0.0, 1.0, (f-SCORCH_BAND_SIZE-BURN_BAND_SIZE)*1000.0);

    f_color = texture(sampler2D(t_screenshot, s_screenshot), uv);
    f_color = mix(f_color, SCORCH_COLOR, scorch*SCORCH_MAX);
    f_color = mix(f_color, BURN_COLOR, burn*BURN_MAX);
    f_color = mix(f_color, vec4(0.0, 0.0, 0.0, 0.0), alpha);
    f_color = mix(f_color, vec4(0.0, 0.0, 0.0, 1.0), iFadeAmount);
}
