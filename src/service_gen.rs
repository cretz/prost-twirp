use prost_build::{Service, ServiceGenerator};

pub struct TwirpServiceGenerator {
    pub embed_client: bool,
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        let prost_twirp_mod = if self.embed_client { "prost_twirp" } else { "::prost_twirp" };
        // Generate the main trait
        buf.push_str("\n");
        service.comments.append_with_indent(0, buf);
        buf.push_str(&format!("pub trait {}<R, E> {{\n", service.name));
        for method in service.methods.iter() {
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!(
                "    fn {}(&self, i: {}) -> Box<::futures::Future<Item={}::ServiceResponse<R, {}>, Error=E>>;\n",
                method.name, method.input_type, prost_twirp_mod, method.output_type));
        }
        buf.push_str("}\n");

        // Add a client constructor for the main trait
        buf.push_str(&format!("\n#[allow(dead_code)]\nimpl {}<{}::HyperResponseHead, {}::HyperClientError> {{\n",
            service.name, prost_twirp_mod, prost_twirp_mod));
        buf.push_str(&format!(
            "    pub fn new_client(client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>, root_url: &str) -> Box<{}<{}::HyperResponseHead, {}::HyperClientError>> {{\n",
            service.name, prost_twirp_mod, prost_twirp_mod
        ));
        buf.push_str(&format!("        Box::new({}Client::new(client, root_url))\n", service.name));
        buf.push_str("    }\n");
        buf.push_str("}\n");

        // Generate a single-item struct for this service's client
        buf.push_str(&format!("\npub struct {}Client({}::HyperClient);\n", service.name, prost_twirp_mod));
        
        // Add constructor and get/set json setting for the client
        buf.push_str(&format!("\n#[allow(dead_code)]\nimpl {}Client {{\n", service.name));
        buf.push_str(&format!(
            "    pub fn new(client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>, root_url: &str) -> {}Client {{\n",
            service.name));
        buf.push_str(&format!("        {}Client({}::HyperClient::new(client, root_url))\n",
            service.name, prost_twirp_mod));
        buf.push_str("    }\n");
        buf.push_str("\n    pub fn json(&self) -> bool { self.0.json }\n");
        buf.push_str("    pub fn set_json(&mut self, json: bool) { self.0.json = json; }\n");
        buf.push_str("}\n");

        // Generate the impl for the client
        buf.push_str(&format!(
            "\nimpl {}<{}::HyperResponseHead, {}::HyperClientError> for {}Client {{\n",
            service.name, prost_twirp_mod, prost_twirp_mod, service.name));
        for method in service.methods.iter() {
            buf.push_str(&format!(
                "    fn {}(&self, i: {}) -> Box<::futures::Future<Item={}::ServiceResponse<{}::HyperResponseHead, {}>, Error={}::HyperClientError>> {{\n",
                method.name, method.input_type, prost_twirp_mod, prost_twirp_mod, method.output_type, prost_twirp_mod));
            buf.push_str(&format!(
                "        self.0.go(\"/twirp/{}.{}/{}\", i)\n", service.package, service.name, method.proto_name));
            buf.push_str("    }\n");
        }
        buf.push_str("}\n");
    }

    fn finalize(&mut self, buf: &mut String) {
        if self.embed_client {
            buf.push_str("\n/// Embedded module from prost_twirp source\nmod prost_twirp {\n");
            for line in include_str!("service_run.rs").lines() {
                buf.push_str(&format!("    {}\n", line));
            }
            buf.push_str("\n}\n");
        }
    }
}