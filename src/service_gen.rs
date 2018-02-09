use prost_build::{Service, ServiceGenerator};

pub struct TwirpServiceGenerator {
    pub embed_client: bool,
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        // Generate the main trait
        buf.push_str("\n");
        service.comments.append_with_indent(0, buf);
        buf.push_str(&format!("pub trait {}<R, E> {{\n", service.name));
        for method in service.methods.iter() {
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!(
                "    fn {}(&self, i: {}) -> Box<::futures::Future<Item=::prost_twirp::ServiceResponse<R, {}>, Error=E>>;\n",
                method.name, method.input_type, method.output_type));
        }
        buf.push_str("}\n");

        // Generate a single-item struct for this service's client
        buf.push_str(&format!("\npub struct {}Client(pub ::prost_twirp::HyperClient);\n", service.name));

        // Generate the impl for the client
        buf.push_str(&format!(
            "\nimpl {}<::prost_twirp::HyperResponseHead, ::prost_twirp::HyperClientError> for {}Client {{\n",
            service.name, service.name));
        for method in service.methods.iter() {
            buf.push_str(&format!(
                "    fn {}(&self, i: {}) -> Box<::futures::Future<Item=::prost_twirp::ServiceResponse<::prost_twirp::HyperResponseHead, {}>, Error=::prost_twirp::HyperClientError>> {{\n",
                method.name, method.input_type, method.output_type));
            buf.push_str(&format!(
                "        self.0.go(\"/twirp/{}.{}/{}\", i)\n", service.package, service.name, method.proto_name));
            buf.push_str("    }\n");
        }
        buf.push_str("}\n");
    }
}