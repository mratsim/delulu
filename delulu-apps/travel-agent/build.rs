fn main() {
    println!("cargo:rerun-if-changed=src/proto/flights.proto");

    prost_build::compile_protos(&["src/proto/flights.proto"], &["."]).unwrap();
}
