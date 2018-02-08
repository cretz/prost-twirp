extern crate prost_build;

use prost_build::{Service, ServiceGenerator};

struct TwirpServiceGenerator;

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        // TODO
    }
}