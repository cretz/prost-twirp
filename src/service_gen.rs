use prost_build::{Method, Service, ServiceGenerator};

#[derive(Default)]
pub struct TwirpServiceGenerator {
    pub embed_client: bool,
    type_aliases_added: bool,
}

impl TwirpServiceGenerator {
    pub fn new() -> TwirpServiceGenerator { Default::default() }

    fn prost_twirp_mod(&self) -> &str { if self.embed_client { "prost_twirp" } else { "::prost_twirp" } }

    fn generate_type_aliases(&mut self, buf: &mut String) {
        if self.type_aliases_added { return; }
        self.type_aliases_added = true;
        buf.push_str(&format!(
            "\n\
            type PTReq<I> = {0}::ServiceRequest<I>;\n\
            type PTRes<O> = Box<::futures::Future<Item={0}::ServiceResponse<O>, Error={0}::ProstTwirpError>>;\n",
            self.prost_twirp_mod()));
    }

    fn generate_main_trait(&self, service: &Service, buf: &mut String) {
        buf.push_str("\n");
        service.comments.append_with_indent(0, buf);
        buf.push_str(&format!("pub trait {} {{", service.name));
        for method in service.methods.iter() {
            buf.push_str("\n");
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!("    {};\n", self.method_sig(method)));
        }
        buf.push_str("}\n");
    }

    fn method_sig(&self, method: &Method) -> String {
        format!("fn {}(&self, i: PTReq<{}>) -> PTRes<{}>",
            method.name, method.input_type, method.output_type)
    }

    fn generate_main_impl(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!(
            "\n\
            impl {0} {{\n    \
                pub fn new_client(client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>, root_url: &str) -> Box<{0}> {{\n        \
                    Box::new({0}Client({1}::HyperClient::new(client, root_url)))\n    \
                }}\n\
            }}\n",
            service.name, self.prost_twirp_mod()));
    }

    fn generate_client_struct(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!(
            "\npub struct {}Client(pub {}::HyperClient);\n",
            service.name, self.prost_twirp_mod()));
    }

    fn generate_client_impl(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!("\nimpl {0} for {0}Client {{", service.name));
        for method in service.methods.iter() {
            buf.push_str(&format!(
                "\n    {} {{\n        \
                    self.0.go(\"/twirp/{}.{}/{}\", i)\n    \
                }}\n", self.method_sig(method), service.package, service.name, method.proto_name));
        }
        buf.push_str("}\n");
    }
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        self.generate_type_aliases(buf);
        self.generate_main_trait(&service, buf);
        self.generate_main_impl(&service, buf);
        self.generate_client_struct(&service, buf);
        self.generate_client_impl(&service, buf);
    }

    fn finalize(&mut self, buf: &mut String) {
        if self.embed_client {
            buf.push_str("\n/// Embedded module from prost_twirp source\n#[allow(dead_code)]\nmod prost_twirp {\n");
            for line in include_str!("service_run.rs").lines() {
                buf.push_str(&format!("    {}\n", line));
            }
            buf.push_str("\n}\n");
        }
    }
}