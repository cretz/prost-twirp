fn main() {
    prost_build::compile_protos(&["service.proto"], &["../"]).unwrap();
}
