
use futures::{Future, Stream};
use futures::future;
use hyper;
use hyper::{Body, Client, Headers, HttpVersion, Method, StatusCode};
use hyper::client::HttpConnector;
use hyper::header::{ContentLength, ContentType};
use prost::{DecodeError, EncodeError, Message};
use serde_json;
use std::error;
use std::fmt;

#[derive(Debug)]
pub struct ServiceResponse<R, T> {
    pub service_response: R,
    pub rpc_response: T,
}

#[derive(Debug)]
pub struct TwirpError {
    pub error_type: String,
    pub msg: String,
    pub meta: serde_json::Value,
    pub err_desc: String,
}

impl TwirpError {
    pub fn new(error_type: &str, msg: &str, meta: serde_json::Value) -> TwirpError {
        TwirpError {
            error_type: error_type.to_string(),
            msg: msg.to_string(),
            meta,
            err_desc: format!("{} - {}", error_type, msg)
        }
    }
}

impl fmt::Display for TwirpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for TwirpError {
    fn description(&self) -> &str {
        &self.err_desc
    }
}

#[derive(Debug)]
pub struct HyperResponseHead {
    pub version: HttpVersion,
    pub headers: Headers,
    pub status: StatusCode,
}

#[derive(Debug)]
pub struct HyperPostResponseError<E: error::Error> {
    pub resp: HyperResponseHead,
    pub body: Vec<u8>,
    pub err: Box<E>,
}

impl<E: error::Error> fmt::Display for HyperPostResponseError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self.err.as_ref(), f)
    }
}

impl<E: error::Error> error::Error for HyperPostResponseError<E> {
    fn description(&self) -> &str { self.err.description() }
}

#[derive(Debug)]
pub enum HyperClientError {
    TwirpError(HyperPostResponseError<TwirpError>),
    JsonDecodeError(HyperPostResponseError<serde_json::Error>),
    ProstEncodeError(EncodeError),
    ProstDecodeError(HyperPostResponseError<DecodeError>),
    HyperError(hyper::Error)
}

#[derive(Debug)]
pub struct HyperClient {
    pub client: Client<HttpConnector, Body>,
    pub root_url: String,
    pub json: bool,
    pub protobuf_content_type: ContentType,
}

impl HyperClient {
    pub fn new(client: Client<HttpConnector, Body>, root_url: &str) -> HyperClient {
        HyperClient {
            client,
            root_url: root_url.trim_right_matches('/').to_string(),
            json: false,
            protobuf_content_type: ContentType("application/protobuf".parse().unwrap()),
        }
    }

    pub fn go<I: Message, O: Message + Default + 'static>(&self, url_path: &str, i: I) ->
            Box<Future<Item=ServiceResponse<HyperResponseHead, O>, Error=HyperClientError>> {
        // Make the URI
        let uri = match format!("{}/{}", self.root_url, url_path.trim_left_matches('/')).parse() {
            Ok(v) => v,
            Err(err) => return Box::new(future::err(HyperClientError::HyperError(hyper::Error::Uri(err))))
        };
        
        // Build the request
        let mut req = hyper::Request::new(Method::Post, uri);
        if self.json {
            req.headers_mut().set(ContentType::json());
            panic!("TODO: JSON serialization");
        } else {
            req.headers_mut().set(self.protobuf_content_type.clone());
            let mut body = Vec::new();
            if let Err(err) = i.encode(&mut body) {
                return Box::new(future::err(HyperClientError::ProstEncodeError(err)));
            }
            req.headers_mut().set(ContentLength(body.len() as u64));
            req.set_body(body);
        }

        // Run the request and map the response
        Box::new(self.client.request(req).
            map_err(|err| HyperClientError::HyperError(err)).
            and_then(|resp| {
                // Copy the non-body parts of the response
                let resp_head = HyperResponseHead {
                    version: resp.version(), headers: resp.headers().clone(), status: resp.status()
                };
                resp.body().concat2().
                    map_err(|err| HyperClientError::HyperError(err)).
                    and_then(move |body| {
                        if resp_head.status.is_success() {
                            match O::decode(body.to_vec()) {
                                Ok(v) => Ok(ServiceResponse { service_response: resp_head, rpc_response: v }),
                                Err(err) => Err(HyperClientError::ProstDecodeError(
                                    HyperPostResponseError { resp: resp_head, body: body.to_vec(), err: Box::new(err) }
                                ))
                            }
                        } else {
                            match serde_json::from_slice::<serde_json::Value>(body.to_vec().as_slice()) {
                                Ok(v) => {
                                    let code = v["code"].as_str();
                                    let twirp_err = TwirpError::new(
                                        code.unwrap_or("<no code>"),
                                        v["msg"].as_str().unwrap_or("<no message>"),
                                        // Put the whole thing as meta if there was no code
                                        if code.is_some() { v["meta"].clone() } else { v.clone() }
                                    );
                                    Err(HyperClientError::TwirpError(
                                        HyperPostResponseError {
                                            resp: resp_head, body: body.to_vec(),
                                            err: Box::new(twirp_err)
                                        }
                                    ))
                                },
                                Err(err) => Err(HyperClientError::JsonDecodeError(
                                    HyperPostResponseError { resp: resp_head, body: body.to_vec(), err: Box::new(err) }
                                ))
                            }
                        }
                    })
            }))
    }
}
