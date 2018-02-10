
use futures::{Future, Stream};
use futures::future;
use hyper;
use hyper::{Body, Client, Headers, HttpVersion, Method, Request, Response, StatusCode, Uri};
use hyper::client::HttpConnector;
use hyper::header::{ContentLength, ContentType};
use prost::{DecodeError, EncodeError, Message};
use serde_json;
use std::error;
use std::fmt;

#[derive(Debug)]
pub struct ServiceRequest<T> {
    pub method: Method,
    pub version: HttpVersion,
    pub headers: Headers,
    pub input: T,
}

impl<T> ServiceRequest<T> {
    pub fn new(input: T) -> ServiceRequest<T> { input.into() }
    pub fn json(&self) -> bool { self.headers.get::<ContentType>() == Some(&ContentType::json()) }
}

impl<T: Message> ServiceRequest<T> {
    pub fn to_hyper_req(&self, uri: Uri) -> Result<Request, ProstTwirpError> {
        let mut req = Request::new(Method::Post, uri);
        req.headers_mut().clone_from(&self.headers);
        if self.json() {
            panic!("TODO: JSON serialization");
        } else {
            let mut body = Vec::new();
            if let Err(err) = self.input.encode(&mut body) {
                return Err(ProstTwirpError::ProstEncodeError(err));
            }
            req.headers_mut().set(ContentLength(body.len() as u64));
            req.set_body(body);
        }
        Ok(req)
    }
}

impl<T> From<T> for ServiceRequest<T> {
    fn from(input: T) -> ServiceRequest<T> {
        let mut headers = Headers::new();
        headers.set(ContentType("application/protobuf".parse().unwrap()));
        ServiceRequest {
            method: Method::Post,
            version: HttpVersion::default(),
            headers: headers,
            input
        }
    }
}

#[derive(Debug)]
pub struct ServiceResponse<T> {
    pub version: HttpVersion,
    pub headers: Headers,
    pub status: StatusCode,
    pub output: T,
}

impl<T: Message + Default + 'static> ServiceResponse<T> {
    pub fn from_hyper_req(resp: Response) -> Box<Future<Item=ServiceResponse<T>, Error=ProstTwirpError>> {
        let version = resp.version();
        let headers = resp.headers().clone();
        let status = resp.status();
        Box::new(resp.body().concat2().map_err(ProstTwirpError::HyperError).and_then(move |body| {
            let resp = ServiceResponse { version, headers, status, output: body.to_vec() };
            if status.is_success() {
                match T::decode(body.to_vec()) {
                    Ok(v) => Ok(ServiceResponse { version, headers: resp.headers, status, output: v }),
                    Err(err) => Err(ProstTwirpError::ProstDecodeError(AfterResponseError { resp, err }))
                }
            } else {
                match TwirpError::from_json_bytes(body.to_vec().as_slice()) {
                    Ok(err) => Err(ProstTwirpError::TwirpError(AfterResponseError { resp, err })),
                    Err(err) => Err(ProstTwirpError::JsonDecodeError(AfterResponseError { resp, err }))
                }
            }
        }))
    }
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

    pub fn from_json(json: serde_json::Value) -> TwirpError {
        let code = json["code"].as_str();
        TwirpError::new(
            code.unwrap_or("<no code>"),
            json["msg"].as_str().unwrap_or("<no message>"),
            // Put the whole thing as meta if there was no code
            if code.is_some() { json["meta"].clone() } else { json.clone() }
        )
    }

    pub fn from_json_bytes(json: &[u8]) -> Result<TwirpError, serde_json::Error> {
        serde_json::from_slice(json).map(&TwirpError::from_json)
    }

    pub fn to_json(&self) -> serde_json::Value {
        panic!("TODO")
    }
}

impl fmt::Display for TwirpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(error::Error::description(self)) }
}

impl error::Error for TwirpError {
    fn description(&self) -> &str { &self.err_desc }
}

#[derive(Debug)]
pub struct AfterResponseError<E: error::Error + Sized> {
    pub resp: ServiceResponse<Vec<u8>>,
    pub err: E,
}

impl<E: error::Error + Sized> fmt::Display for AfterResponseError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::Display::fmt(&self.err, f) }
}

impl<E: error::Error + Sized> error::Error for AfterResponseError<E> {
    fn description(&self) -> &str { self.err.description() }
}

#[derive(Debug)]
pub enum ProstTwirpError {
    TwirpError(AfterResponseError<TwirpError>),
    JsonDecodeError(AfterResponseError<serde_json::Error>),
    ProstEncodeError(EncodeError),
    ProstDecodeError(AfterResponseError<DecodeError>),
    HyperError(hyper::Error)
}

#[derive(Debug)]
pub struct HyperClient {
    pub client: Client<HttpConnector, Body>,
    pub root_url: String,
    pub json: bool,
}

impl HyperClient {
    pub fn new(client: Client<HttpConnector, Body>, root_url: &str) -> HyperClient {
        HyperClient {
            client,
            root_url: root_url.trim_right_matches('/').to_string(),
            json: false,
        }
    }

    pub fn go<I, O>(&self, path: &str, req: ServiceRequest<I>) -> Box<Future<Item=ServiceResponse<O>, Error=ProstTwirpError>>
            where I: Message, O: Message + Default + 'static {
        // Build the URI
        let uri = match format!("{}/{}", self.root_url, path.trim_left_matches('/')).parse() {
            Err(err) => return Box::new(future::err(ProstTwirpError::HyperError(hyper::Error::Uri(err)))),
            Ok(v) => v,
        };
        // Build the request
        let hyper_req = match req.to_hyper_req(uri) {
            Err(err) => return Box::new(future::err(err)),
            Ok(v) => v
        };

        // Run the request and map the response
        Box::new(self.client.request(hyper_req).
            map_err(ProstTwirpError::HyperError).
            and_then(ServiceResponse::from_hyper_req))
    }
}