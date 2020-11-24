use std::fs::{read_to_string, write};

const FILENAME: &str = "resources/bg.vert";
const SPIRV_FILENAME: &str = "resources/bg.vert.spv";

fn main() {
    let mut compiler = shaderc::Compiler::new().expect("Failed to create compiler");
    println!("cargo:rerun-if-changed={:?}", FILENAME);

    let data = read_to_string(FILENAME).expect("Failed to read shader");
    let compiled = compiler
        .compile_into_spirv(&data, shaderc::ShaderKind::Vertex, FILENAME, "main", None)
        .expect("Failed to compile shader");
    write(SPIRV_FILENAME, compiled.as_binary_u8()).expect("Failed to write Spirv");
}
