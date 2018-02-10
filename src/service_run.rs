
use futures::{Future, Stream};
use futures::future;
use hyper;
use hyper::{Body, Client, Headers, HttpVersion, Method, Request, Response, StatusCode, Uri};
use hyper::client::HttpConnector;
use hyper::header::{ContentLength, ContentType};
use hyper::server::Service;
use prost::{DecodeError, EncodeError, Message};
use serde_json;
use std::collections::HashMap;

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

impl<T: Message + Default + 'static> ServiceRequest<T> {
    pub fn from_hyper_req(req: Request) -> Box<Future<Item=ServiceRequest<T>, Error=ProstTwirpError>> {
        let method = req.method().clone();
        let version = req.version();
        let json = req.headers().get::<ContentType>() == Some(&ContentType::json());
        let headers = req.headers().clone();
        Box::new(req.body().concat2().map_err(ProstTwirpError::HyperError).and_then(move |body| {
            if json {
                panic!("TODO: JSON serialization");
            } else {
                match T::decode(body.to_vec()) {
                    Ok(v) => Ok(ServiceRequest { method, version, headers, input: v }),
                    Err(err) => Err(ProstTwirpError::AfterBodyError {
                        body: body.to_vec(), method: Some(method), version, headers, status: None,
                        err: Box::new(ProstTwirpError::ProstDecodeError(err))
                    })
                }
            }
        }))
    }

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

impl<T> ServiceResponse<T> {
    pub fn new(input: T) -> ServiceResponse<T> { input.into() }
    pub fn json(&self) -> bool { self.headers.get::<ContentType>() == Some(&ContentType::json()) }
}

impl<T: Message + Default + 'static> ServiceResponse<T> {
    pub fn from_hyper_resp(resp: Response) -> Box<Future<Item=ServiceResponse<T>, Error=ProstTwirpError>> {
        let version = resp.version();
        let headers = resp.headers().clone();
        let status = resp.status();
        Box::new(resp.body().concat2().map_err(ProstTwirpError::HyperError).and_then(move |body| {
            if status.is_success() {
                match T::decode(body.to_vec()) {
                    Ok(v) => Ok(ServiceResponse { version, headers, status, output: v }),
                    Err(err) => Err(ProstTwirpError::AfterBodyError {
                        body: body.to_vec(), method: None, version, headers, status: Some(status),
                        err: Box::new(ProstTwirpError::ProstDecodeError(err))
                    })
                }
            } else {
                match TwirpError::from_json_bytes(body.to_vec().as_slice()) {
                    Ok(err) => Err(ProstTwirpError::AfterBodyError {
                        body: body.to_vec(), method: None, version, headers, status: Some(status),
                        err: Box::new(ProstTwirpError::TwirpError(err))
                    }),
                    Err(err) => Err(ProstTwirpError::AfterBodyError {
                        body: body.to_vec(), method: None, version, headers, status: Some(status),
                        err: Box::new(ProstTwirpError::JsonDecodeError(err))
                    })
                }
            }
        }))
    }

    pub fn to_hyper_resp(&self) -> Result<Response, ProstTwirpError> {
        let mut resp = Response::new().with_status(self.status).with_headers(self.headers.clone());
        if self.json() {
            panic!("TODO: JSON serialization");
        } else {
            let mut body = Vec::new();
            if let Err(err) = self.output.encode(&mut body) {
                return Err(ProstTwirpError::ProstEncodeError(err));
            }
            resp.headers_mut().set(ContentLength(body.len() as u64));
            resp.set_body(body);
        }
        Ok(resp)
    }
}

impl<T> From<T> for ServiceResponse<T> {
    fn from(output: T) -> ServiceResponse<T> {
        let mut headers = Headers::new();
        headers.set(ContentType("application/protobuf".parse().unwrap()));
        ServiceResponse {
            version: HttpVersion::default(),
            headers: headers,
            status: StatusCode::Ok,
            output
        }
    }
}

#[derive(Debug)]
pub struct TwirpError {
    pub error_type: String,
    pub msg: String,
    pub meta: Option<serde_json::Value>,
}

impl TwirpError {
    pub fn new(error_type: &str, msg: &str) -> TwirpError {
        TwirpError { error_type: error_type.to_string(), msg: msg.to_string(), meta: None }
    }

    pub fn from_json(json: serde_json::Value) -> TwirpError {
        let error_type = json["error_type"].as_str();
        TwirpError {
            error_type: error_type.unwrap_or("<no code>").to_string(),
            msg: json["msg"].as_str().unwrap_or("<no message>").to_string(),
            // Put the whole thing as meta if there was no type
            meta: if error_type.is_some() { json.get("meta").map(|v| v.clone()) } else { Some(json.clone()) },
        }
    }

    pub fn from_json_bytes(json: &[u8]) -> serde_json::Result<TwirpError> {
        serde_json::from_slice(json).map(&TwirpError::from_json)
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut props = serde_json::map::Map::new();
        props.insert("error_type".to_string(), serde_json::Value::String(self.error_type.clone()));
        props.insert("msg".to_string(), serde_json::Value::String(self.msg.clone()));
        if let Some(ref meta) = self.meta { props.insert("meta".to_string(), meta.clone()); }
        serde_json::Value::Object(props)
    }

    pub fn to_json_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&self.to_json())
    }
}

#[derive(Debug)]
pub enum ProstTwirpError {
    TwirpError(TwirpError),
    JsonDecodeError(serde_json::Error),
    ProstEncodeError(EncodeError),
    ProstDecodeError(DecodeError),
    HyperError(hyper::Error),
    AfterBodyError {
        body: Vec<u8>,
        /// Only present for server errors
        method: Option<Method>,
        version: HttpVersion,
        headers: Headers,
        // Only present for client errors
        status: Option<StatusCode>,
        err: Box<ProstTwirpError>,
    }
}

impl ProstTwirpError {
    pub fn root_err(self) -> ProstTwirpError {
        match self {
            ProstTwirpError::AfterBodyError { err, .. } => err.root_err(),
            _ => self
        }
    }
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
            where I: Message + Default + 'static, O: Message + Default + 'static {
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
            and_then(ServiceResponse::from_hyper_resp))
    }
}

type HyperCallback = Box<Fn(Request) -> Box<Future<Item=Response, Error=ProstTwirpError>> + 'static>;
// type HyperCallback = 

pub struct HyperServer {
    json: bool,
    // Key is /<package>.<Service>/<Method>
    methods: HashMap<String, HyperCallback>,
}

impl HyperServer {
    pub fn add_method<I, O, F>(&mut self, path: &str, cb: &'static F)
            where I: Message + Default + 'static,
                  O: Message + Default + 'static,
                  F: Fn(ServiceRequest<I>) -> Box<Future<Item=ServiceResponse<O>, Error=ProstTwirpError>> + 'static {
        self.methods.insert(path.to_string(), Box::new(move |req| {
            Box::new(ServiceRequest::from_hyper_req(req).and_then(cb).and_then(|v| v.to_hyper_resp()))
        }));
    }

    pub fn err_resp(&self, status: StatusCode, err: TwirpError) -> Response {
        let body = err.to_json_bytes().unwrap_or_else(|_| "{}".as_bytes().to_vec());
        Response::new().
            with_status(status).
            with_header(ContentType::json()).
            with_header(ContentLength(body.len() as u64)).
            with_body(body)
    }
}

impl Service for HyperServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        if req.method() != &Method::Post {
            Box::new(future::ok(self.err_resp(
                StatusCode::MethodNotAllowed, TwirpError::new("bad_method", "Must be 'POST'"))))
        } else {
            match self.methods.get(req.path()) {
                None => Box::new(future::ok(self.err_resp(
                    StatusCode::NotFound, TwirpError::new("not_found", "Not found")))),
                Some(cb) => Box::new(cb(req).or_else(|err| {
                    let (status, twirp_err) = match err.root_err() {
                        // TODO
                        _ => (StatusCode::InternalServerError, TwirpError::new("internal_err", "Internal Error"))
                    };
                    let body = twirp_err.to_json_bytes().unwrap_or_else(|_| "{}".as_bytes().to_vec());
                    Ok(Response::new().
                        with_status(status).
                        with_header(ContentType::json()).
                        with_header(ContentLength(body.len() as u64)).
                        with_body(body))
                }))
            }
        }
    }
}