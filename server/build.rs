fn main() {
    let proto = "../protocol/aqua.proto";
    let include = "../protocol";
    println!("cargo:rerun-if-changed={proto}");
    let mut config = prost_build::Config::new();
    config
        .compile_protos(&[proto], &[include])
        .expect("failed to compile Aqua protocol schema");
}
