fn main() {
    println!("cargo:rerun-if-changed=src/proto/flights.proto");
    println!("cargo:rerun-if-changed=src/proto/cookies.proto");

    prost_build::compile_protos(
        &["src/proto/flights.proto", "src/proto/cookies.proto"],
        &["."],
    )
    .unwrap();
}
