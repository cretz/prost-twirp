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

    fn generate_type_aliases(&mut self, buf: &mut String) {
        let mod_path = self.prost_twirp_path();
        buf.push_str(
            quote! {
                pub use #mod_path::ServiceRequest;
                pub use #mod_path::PTRes;
            }
            .to_string()
            .as_str(),
        );
    }

    fn generate_main_trait(&self, service: &Service, buf: &mut String) {
        // This is done with strings rather than tokens because Prost provides functions that
        // return doc comments as strings.
        buf.push('\n');
        service.comments.append_with_indent(0, buf);
        buf.push_str(&format!(
            "pub trait {}: Send + Sync + 'static {{",
            service.name
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!("    {};\n", self.method_sig_tokens(method)));
        }
        buf.push_str("}\n");
    }

    fn method_sig_tokens(&self, method: &Method) -> TokenStream {
        let name = format_ident!("{}", method.name);
        let prost_twirp = self.prost_twirp_path();
        let input_type = format_ident!("{}", method.input_type);
        let output_type = format_ident!("{}", method.output_type);
        quote! {
            fn #name(&self, request: #prost_twirp::ServiceRequest<#input_type>)
                -> #prost_twirp::PTRes<#output_type>
        }
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
                #[allow(clippy::type_complexity)]
                pub fn new_server<T: #service_name>(v: T)
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
        buf.push_str(&s);
    }

    fn generate_client(&self, service: &Service, buf: &mut String) {
        let prost_twirp_path = self.prost_twirp_path();
        let client_name = self.client_name_ident(service);
        let service_name = self.service_name_ident(service);
        let methods: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                let method_sig = self.method_sig_tokens(method);
                let url = self.method_url(service, method);
                quote! {
                    #method_sig {
                        self.0.go(#url, request)
                    }
                }
            })
            .collect();
        let toks = quote! {
            pub struct #client_name(pub #prost_twirp_path::HyperClient);

            impl #service_name for #client_name {
                #(#methods)*
            }
        };
        buf.push_str(toks.to_string().as_str());
    }

    fn generate_server(&self, service: &Service, buf: &mut String) {
        let service_name = self.service_name_ident(service);
        let server_name = self.server_name_ident(service);
        let mod_path = self.prost_twirp_path();
        let match_arms: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                let path = self.method_url(service, method);
                let method_name = format_ident!("{}", method.name);
                quote! {
                    #path => Box::pin(async move {
                        let req = #mod_path::ServiceRequest::from_hyper_request(req).await?;
                        static_service.#method_name(req).await?.to_hyper_response()
                    }),
                }
            })
            .collect();
        let toks = quote! {
            pub struct #server_name<T: #service_name>(::std::sync::Arc<T>);

            impl<T: #service_name> #mod_path::HyperService for #server_name<T> {
                fn handle(&self, req: ::hyper::Request<::hyper::Body>)
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
            }
        };
        buf.push_str(toks.to_string().as_str());
    }

    fn method_url(&self, service: &Service, method: &Method) -> String {
        // https://twitchtv.github.io/twirp/docs/routing.html#http-routes
        format!(
            "/twirp/{}.{}/{}",
            service.package, service.proto_name, method.proto_name
        )
    }
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        self.generate_type_aliases(buf);
        self.generate_main_trait(&service, buf);
        self.generate_main_impl(&service, buf);
        self.generate_client(&service, buf);
        self.generate_server(&service, buf);
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
