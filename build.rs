use std::fs::{read_to_string, write};

const ENV_WITH_DEFAULT: &[(&str, &str)] = &[
    ("DATADIR", "dist"),
    ("PAM_SERVICE", "system-auth"),
];

const RESOURCES: &str = "resources";
const VERTEX_SHADER_GLOB: &str = "*.vert";
const VERTEX_SPIRV_EXTENSION: &str = "vert.spv";
const FRAGMENT_SHADER_GLOB: &str = "*.frag";
const FRAGMENT_SPIRV_EXTENSION: &str = "frag.spv";

fn vertex_shader_files() -> impl Iterator<Item = std::path::PathBuf> {
    glob::glob(&format!("{}/{}", RESOURCES, VERTEX_SHADER_GLOB))
        .expect("Failed to parse shader file glob")
        .map(|r| r.expect("Failed to read path"))
}

fn fragment_shader_files() -> impl Iterator<Item = std::path::PathBuf> {
    glob::glob(&format!("{}/{}", RESOURCES, FRAGMENT_SHADER_GLOB))
        .expect("Failed to parse shader file glob")
        .map(|r| r.expect("Failed to read path"))
}

fn compile(compiler: &mut shaderc::Compiler, file: &std::path::Path, kind: shaderc::ShaderKind) {
    println!("cargo:rerun-if-changed={:?}", file);

    let data = read_to_string(file).expect("Failed to read shader");
    let compiled = compiler
        .compile_into_spirv(&data, kind, &file.to_string_lossy(), "main", None)
        .expect("Failed to compile shader");

    let output = match kind {
        shaderc::ShaderKind::Vertex => file.with_extension(VERTEX_SPIRV_EXTENSION),
        shaderc::ShaderKind::Fragment => file.with_extension(FRAGMENT_SPIRV_EXTENSION),
        _ => unreachable!(),
    };
    write(output, compiled.as_binary_u8()).expect("Failed to write Spirv");
}

fn main() {
    let mut compiler = shaderc::Compiler::new().expect("Failed to create compiler");
    for file in vertex_shader_files() {
        compile(&mut compiler, &file, shaderc::ShaderKind::Vertex);
    }
    for file in fragment_shader_files() {
        compile(&mut compiler, &file, shaderc::ShaderKind::Fragment);
    }

    if std::env::var("PROFILE").unwrap() != "release" {
        for (var, default) in ENV_WITH_DEFAULT {
            println!("cargo:rustc-env={}={}", var, default);
        }
    }
}
