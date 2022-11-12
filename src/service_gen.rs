use quote::{format_ident, quote};

use prost_build::{Method, Service, ServiceGenerator};

#[derive(Default)]
pub struct TwirpServiceGenerator {
    pub embed_client: bool,
    type_aliases_generated: bool,
}

impl TwirpServiceGenerator {
    pub fn new() -> TwirpServiceGenerator {
        Default::default()
    }

    fn prost_twirp_mod(&self) -> &str {
        if self.embed_client {
            "prost_twirp"
        } else {
            "::prost_twirp"
        }
    }

    fn generate_imports(&self, buf: &mut String) {
        buf.push_str("// hello!\n");
        buf.push_str(
            quote! {
                // use hyper::service::Service;
                use futures::TryFutureExt;
            }
            .to_string()
            .as_str(),
        );
    }

    fn generate_type_aliases(&mut self, buf: &mut String) {
        if !self.type_aliases_generated {
            self.type_aliases_generated = true;
            buf.push_str(&format!(
                "\n\
                pub type PTReq<I> = {0}::PTReq<I>;\n\
                pub type PTRes<O> = {0}::PTRes<O>;\n",
                self.prost_twirp_mod()
            ));
        }
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
        format!(
            "fn {0}(&self, i: {1}::PTReq<{2}>) -> {1}::PTRes<{3}>",
            method.name,
            self.prost_twirp_mod(),
            method.input_type,
            method.output_type
        )
    }

    fn generate_main_impl(&self, service: &Service, buf: &mut String) {
        let service_name = format_ident!("{}", &service.name);
        let client_name = format_ident!("{}Client", &service.name);
        let server_name = format_ident!("{}Server", &service.name);
        let mod_name = format_ident!("prost_twirp");
        let mod_path = if self.embed_client {
            quote! { crate::#mod_name }
        } else {
            quote! { ::#mod_name }
        };
        let s = quote! {
            impl dyn #service_name {
                pub fn new_client(
                        client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>,
                        root_url: &str)
                    -> Box<dyn #service_name> {
                    Box::new(#client_name(#mod_path::HyperClient::new(client, root_url)))
                }

                pub fn new_server<T: 'static + #service_name>(v: T)
                    -> Box<dyn (::hyper::service::Service<::hyper::Request<::hyper::body::Body>,
                        Response=::hyper::Response<::hyper::body::Body>,
                        Error=::hyper::Error,
                        Future=Box<dyn (::futures::Future<Output=::hyper::Response<::hyper::body::Body>>)>>)> {
                    Box::new(#mod_path::HyperServer::new(#server_name(::std::sync::Arc::new(v))))
                }
            }
        }
        .to_string();
        println!("{s}");
        buf.push_str(&s);
    }

    fn generate_client_struct(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!(
            "\npub struct {}Client(pub {}::HyperClient);\n",
            service.name,
            self.prost_twirp_mod()
        ));
    }

    fn generate_client_impl(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!("\nimpl {0} for {0}Client {{", service.name));
        for method in service.methods.iter() {
            buf.push_str(&format!(
                "\n    {} {{\n        \
                    self.0.go(\"/twirp/{}.{}/{}\", i)\n    \
                }}\n",
                self.method_sig(method),
                service.package,
                service.proto_name,
                method.proto_name
            ));
        }
        buf.push_str("}\n");
    }

    fn generate_server_struct(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!(
            "\npub struct {0}Server<T: 'static + {0}>(::std::sync::Arc<T>);\n",
            service.name
        ));
    }

    fn generate_server_impl(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!(
            "\n\
            impl<T: 'static + {0}> {1}::HyperService for {0}Server<T> {{\n    \
                fn handle(&self, req: {1}::ServiceRequest<Vec<u8>>) -> {1}::PTRes<Vec<u8>> {{\n        \
                    let static_service = self.0.clone();\n        \
                    match (req.method.clone(), req.uri.path()) {{",
            service.name, self.prost_twirp_mod()));
        // Make match arms for each type
        for method in service.methods.iter() {
            buf.push_str(&format!(
                "\n            \
                (::hyper::Method::POST, \"/twirp/{}.{}/{}\") =>\n                \
                    Box::pin(::futures::future::ready(req.to_proto()).and_then(move |v| static_service.{}(v)).and_then(|v| v.to_proto_raw())),",
                service.package, service.proto_name, method.proto_name, method.name));
        }
        // Final 404 arm and end fn
        buf.push_str(&format!(
            "\n            \
                        _ => Box::new(::futures::future::ok({0}::TwirpError::new(::hyper::StatusCode::NOT_FOUND, \"not_found\", \"Not found\").to_resp_raw()))\n        \
                    }}\n    \
                }}\n\
            }}",
            self.prost_twirp_mod()));
    }
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        self.generate_imports(buf);
        self.generate_type_aliases(buf);
        self.generate_main_trait(&service, buf);
        self.generate_main_impl(&service, buf);
        self.generate_client_struct(&service, buf);
        self.generate_client_impl(&service, buf);
        self.generate_server_struct(&service, buf);
        self.generate_server_impl(&service, buf);
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
