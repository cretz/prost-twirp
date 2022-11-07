fn main() {
    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(prost_twirp::TwirpServiceGenerator::new()));
    conf.compile_protos(&["service.proto"], &["../"]).unwrap();
}
