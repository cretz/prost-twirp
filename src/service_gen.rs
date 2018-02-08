use prost_build::{Service, ServiceGenerator};

pub struct TwirpServiceGenerator {
    pub embed_client: bool,
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        // We need to generate a trait for the service
        buf.push_str("\n");
        service.comments.append_with_indent(0, buf);
        buf.push_str(&format!("pub trait {}<R, E> {{\n", service.name));
        for method in service.methods {
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!(
                "    fn {}(&self, i: {}) -> Box<futures::Future<Item=prost_twirp::ServiceResponse<R, {}>, Error=E>>;\n",
                method.name, method.input_type, method.output_type));
        }
        buf.push_str("}\n");
    }
}