//! Generate service code from a service definition.

// Guidelines for generated code:
//
// Use fully-qualified paths, to reduce the chance of clashing with
// user provided names.

use proc_macro2::TokenStream;
use prost_build::{Method, Service, ServiceGenerator};
use quote::{format_ident, quote};

#[derive(Default)]
pub struct TwirpServiceGenerator {
    pub embed_client: bool,
}

impl TwirpServiceGenerator {
    pub fn new() -> TwirpServiceGenerator {
        Default::default()
    }

    fn generate_imports(&self, buf: &mut String) {
        // None at present, but kept as a place to add any that are needed.
        buf.push_str(quote! {}.to_string().as_str());
    }

    fn generate_type_aliases(&mut self, buf: &mut String) {
        buf.push_str(&format!(
            "\n\
                pub type PTReq<I> = {0}::PTReq<I>;\n\
                pub type PTRes<O> = {0}::PTRes<O>;\n",
            self.prost_twirp_mod()
        ));
    }

    fn generate_main_trait(&self, service: &Service, buf: &mut String) {
        buf.push('\n');
        service.comments.append_with_indent(0, buf);
        buf.push_str(&format!("pub trait {} {{", service.name));
        for method in service.methods.iter() {
            buf.push('\n');
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
            quote! { #mod_name }
        } else {
            quote! { ::#mod_name }
        }
    }

    fn prost_twirp_mod(&self) -> &str {
        if self.embed_client {
            "prost_twirp"
        } else {
            "::prost_twirp"
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
                #[allow(dead_code)]
                pub fn new_client(
                        client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>,
                        root_url: &str)
                    -> Box<dyn #service_name> {
                    Box::new(#client_name(#mod_path::HyperClient::new(client, root_url)))
                }

                /// Make a new server for the service.
                ///
                /// Method calls are forwarded to the implementation in `v`.
                ///
                /// Due to <https://github.com/hyperium/hyper/issues/2051> this can't be directly
                /// passed to `Service::serve`.
                #[allow(dead_code)]
                pub fn new_server<T: #service_name + Send + Sync +'static>(v: T)
                    -> Box<dyn (
                        ::hyper::service::Service<
                            ::hyper::Request<::hyper::Body>,
                            Response=::hyper::Response<::hyper::Body>,
                            Error=::hyper::Error,
                            Future=::std::pin::Pin<Box<
                                dyn (::futures::Future<
                                    Output=Result<::hyper::Response<::hyper::Body>,
                                    ::hyper::Error>>) + Send
                            >>
                        >
                    ) + Send + Sync>
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
                "\n    {method_sig} {{\n        \
                    self.0.go(\"{url}\", i)\n    \
                }}\n",
                method_sig = self.method_sig(method),
                url = self.method_url(service, method)
            ));
        }
        buf.push_str("}\n");
    }

    fn generate_server_struct(&self, service: &Service, buf: &mut String) {
        buf.push_str(&format!(
            "\npub struct {0}Server<T: {0} + Send + Sync + 'static>(::std::sync::Arc<T>);\n",
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
                -> ::std::pin::Pin<Box<
                    dyn ::futures::Future<
                        Output = Result<::hyper::Response<::hyper::Body>,
                            #mod_path::ProstTwirpError>> + Send + 'static>> {
                let static_service = ::std::sync::Arc::clone(&self.0);
                match req.uri().path() {
                    #(#match_arms),*
                    _ => Box::pin(::futures::future::ok(
                        #mod_path::ProstTwirpError::NotFound.into_hyper_response().unwrap()
                    ))
                }
            }
        };
        let service_impl = quote! {
            impl<T: #service_name + Send + Sync + 'static> #mod_path::HyperService for #server_name<T> {
                #handle_method
            }
        };
        buf.push_str(service_impl.to_string().as_str());
    }

    fn method_server_impl_match_case(&self, service: &Service, method: &Method) -> TokenStream {
        let path = self.method_url(service, method);
        let method_name = format_ident!("{}", method.name);
        let mod_path = self.prost_twirp_path();
        quote! {
            #path => Box::pin( async move {
                let req = #mod_path::ServiceRequest::from_hyper_request(req).await?;
                let resp = static_service.#method_name(req).await?;
                resp.to_hyper_response()
            }),
        }
    }

    fn method_url(&self, service: &Service, method: &Method) -> String {
        format!(
            "/twirp/{}/{}.{}",
            service.package, service.proto_name, method.proto_name
        )
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
