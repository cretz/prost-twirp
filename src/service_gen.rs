//! Generate service code from a service definition.

use proc_macro2::TokenStream;
use prost_build::{Method, Service, ServiceGenerator};
use quote::{format_ident, quote};

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
        let mod_path = self.prost_twirp_path();
        buf.push_str(
            quote! {
                use std::pin::Pin;
                use std::sync::Arc;

                use futures::{self, future, Future, TryFutureExt};
                use hyper::{Request, Response, Body};
                use #mod_path::{ProstTwirpError};
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

    fn service_name_ident(&self, service: &Service) -> proc_macro2::Ident {
        format_ident!("{}", service.name)
    }

    fn client_name_ident(&self, service: &Service) -> proc_macro2::Ident {
        format_ident!("{}Client", &service.name)
    }

    fn server_name_ident(&self, service: &Service) -> proc_macro2::Ident {
        format_ident!("{}Server", &service.name)
    }

    fn prost_twirp_path(&self) -> proc_macro2::TokenStream {
        let mod_name = format_ident!("prost_twirp");
        if self.embed_client {
            quote! { crate::#mod_name }
        } else {
            quote! { ::#mod_name }
        }
    }

    fn generate_main_impl(&self, service: &Service, buf: &mut String) {
        let service_name = self.service_name_ident(service);
        let client_name = self.client_name_ident(service);
        let server_name = self.server_name_ident(service);
        let mod_path = self.prost_twirp_path();
        let s = quote! {
            impl dyn #service_name {
                /// Construct a new client stub for the service.
                ///
                /// The client's implementation of the trait methods will make HTTP requests to the
                /// server addressed by `client`.
                pub fn new_client(
                        client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>,
                        root_url: &str)
                    -> Box<dyn #service_name> {
                    Box::new(#client_name(#mod_path::HyperClient::new(client, root_url)))
                }

                /// Make a new server for the service.
                ///
                /// Method calls are forwarded to the implementation in `v`.
                pub fn new_server<T: 'static + #service_name>(v: T)
                    -> Box<dyn (::hyper::service::Service<
                            ::hyper::Request<Body>,
                            Response=::hyper::Response<Body>,
                            Error=::hyper::Error,
                            Future=Pin<Box<dyn (Future<Output=Result<Response<Body>, ::hyper::Error>>)>>
                            >)>
                         {
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
        let service_name = self.service_name_ident(service);
        let server_name = self.server_name_ident(service);
        let mod_path = self.prost_twirp_path();
        let match_arms: Vec<_> = service
            .methods
            .iter()
            .map(|method| self.method_server_impl_match_case(service, method))
            .collect();
        let handle_method = quote! {
            fn handle(&self, req: hyper::Request<hyper::Body>)
                -> Pin<Box<dyn Future<Output = Result<Response<Body>, ProstTwirpError>>>> {
                let static_service = Arc::clone(&self.0);
                match req.uri().path() {
                    #(#match_arms),*
                    _ => Box::pin(::futures::future::ok(
                        // TODO: Specific NotFound error in the library?
                        #mod_path::TwirpError::new(
                            ::hyper::StatusCode::NOT_FOUND,
                            "not_found",
                            "Not found")
                        .to_hyper_response()))
                }
            }
        };
        let service_impl = quote! {
            impl<T: 'static + #service_name> #mod_path::HyperService for #server_name<T> {
                #handle_method
            }
        };
        buf.push_str(service_impl.to_string().as_str());
    }

    fn method_server_impl_match_case(&self, service: &Service, method: &Method) -> TokenStream {
        let path = format!(
            "/twirp/{}.{}/{}",
            service.package, service.proto_name, method.proto_name
        );
        let method_name = format_ident!("{}", method.name);
        let mod_path = self.prost_twirp_path();
        quote! {
            #path => Box::pin(
                #mod_path::ServiceRequest::from_hyper_request(req)
                    .and_then(move |v| static_service.#method_name(v))
                    .and_then(|v| future::ready(v.to_hyper_response()))),
        }
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
