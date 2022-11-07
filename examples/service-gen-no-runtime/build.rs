fn main() {
    let mut conf = prost_build::Config::new();
    let mut gen = prost_twirp::TwirpServiceGenerator::new();
    gen.embed_client = true;
    conf.service_generator(Box::new(gen));
    conf.compile_protos(&["service.proto"], &["../"]).unwrap();
}
