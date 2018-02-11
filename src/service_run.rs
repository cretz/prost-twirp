
use futures::{Future, Stream};
use futures::future;
use hyper;
use hyper::{Body, Client, Headers, HttpVersion, Method, Request, Response, StatusCode, Uri};
use hyper::client::HttpConnector;
use hyper::header::{ContentLength, ContentType};
use hyper::server::Service;
use prost::{DecodeError, EncodeError, Message};
use serde_json;

#[derive(Debug)]
pub struct ServiceRequest<T> {
    pub uri: Uri,
    pub method: Method,
    pub version: HttpVersion,
    pub headers: Headers,
    pub input: T,
}

pub type FutReq<T> = Box<Future<Item=ServiceRequest<T>, Error=ProstTwirpError>>;

impl<T> ServiceRequest<T> {
    pub fn new(input: T) -> ServiceRequest<T> {
        let mut headers = Headers::new();
        headers.set(ContentType("application/protobuf".parse().unwrap()));
        ServiceRequest {
            uri: Default::default(),
            method: Method::Post,
            version: HttpVersion::default(),
            headers: headers,
            input
        }
    }
    
    pub fn clone_with_input<U>(&self, input: U) -> ServiceRequest<U> {
        ServiceRequest { uri: self.uri.clone(), method: self.method.clone(), version: self.version,
            headers: self.headers.clone(), input }
    }

    pub fn json(&self) -> bool { self.headers.get::<ContentType>() == Some(&ContentType::json()) }
}

impl<T: Message + Default + 'static> From<T> for ServiceRequest<T> {
    fn from(v: T) -> ServiceRequest<T> { ServiceRequest::new(v) }
}

impl ServiceRequest<Vec<u8>> {
    pub fn from_hyper_raw(req: Request) -> FutReq<Vec<u8>> {
        let uri = req.uri().clone();
        let method = req.method().clone();
        let version = req.version();
        let headers = req.headers().clone();
        Box::new(req.body().concat2().map_err(ProstTwirpError::HyperError).map(move |body| {
            ServiceRequest { uri, method, version, headers, input: body.to_vec() }
        }))
    }

    pub fn to_hyper_raw(&self) -> Request {
        let mut req = Request::new(Method::Post, self.uri.clone());
        req.headers_mut().clone_from(&self.headers);
        req.headers_mut().set(ContentLength(self.input.len() as u64));
        req.set_body(self.input.clone());
        req
    }

    pub fn body_err(&self, err: ProstTwirpError) -> ProstTwirpError {
        ProstTwirpError::AfterBodyError {
            body: self.input.clone(), method: Some(self.method.clone()), version: self.version,
            headers: self.headers.clone(), status: None, err: Box::new(err)
        }
    }

    pub fn to_proto<T: Message + Default + 'static>(&self) -> Result<ServiceRequest<T>, ProstTwirpError> {
        match T::decode(&self.input) {
            Ok(v) => Ok(self.clone_with_input(v)),
            Err(err) => Err(self.body_err(ProstTwirpError::ProstDecodeError(err)))
        }
    }
}

impl<T: Message + Default + 'static> ServiceRequest<T> {
    pub fn to_proto_raw(&self) -> Result<ServiceRequest<Vec<u8>>, ProstTwirpError> {
        let mut body = Vec::new();
        if let Err(err) = self.input.encode(&mut body) {
            Err(ProstTwirpError::ProstEncodeError(err))
        } else {
            Ok(self.clone_with_input(body))
        }
    }

    pub fn from_hyper_proto(req: Request) -> FutReq<T> {
        Box::new(ServiceRequest::from_hyper_raw(req).and_then(|v| v.to_proto()))
    }

    pub fn to_hyper_proto(&self) -> Result<Request, ProstTwirpError> {
        self.to_proto_raw().map(|v| v.to_hyper_raw())
    }
}

#[derive(Debug)]
pub struct ServiceResponse<T> {
    pub version: HttpVersion,
    pub headers: Headers,
    pub status: StatusCode,
    pub output: T,
}

pub type FutResp<T> = Box<Future<Item=ServiceResponse<T>, Error=ProstTwirpError>>;

impl<T> ServiceResponse<T> {
    pub fn new(output: T) -> ServiceResponse<T> { 
        let mut headers = Headers::new();
        headers.set(ContentType("application/protobuf".parse().unwrap()));
        ServiceResponse {
            version: HttpVersion::default(),
            headers: headers,
            status: StatusCode::Ok,
            output
        }
    }
    
    pub fn clone_with_output<U>(&self, output: U) -> ServiceResponse<U> {
        ServiceResponse { version: self.version, headers: self.headers.clone(), status: self.status, output }
    }

    pub fn json(&self) -> bool { self.headers.get::<ContentType>() == Some(&ContentType::json()) }
}

impl<T: Message + Default + 'static> From<T> for ServiceResponse<T> {
    fn from(v: T) -> ServiceResponse<T> { ServiceResponse::new(v) }
}

impl ServiceResponse<Vec<u8>> {
    pub fn from_hyper_raw(resp: Response) -> FutResp<Vec<u8>> {
        let version = resp.version();
        let headers = resp.headers().clone();
        let status = resp.status();
        Box::new(resp.body().concat2().map_err(ProstTwirpError::HyperError).map(move |body| {
            ServiceResponse { version, headers, status, output: body.to_vec() }
        }))
    }

    pub fn to_hyper_raw(&self) -> Response {
        Response::new().
            with_status(self.status).
            with_headers(self.headers.clone()).
            with_header(ContentLength(self.output.len() as u64)).
            with_body(self.output.clone())
    }

    pub fn body_err(&self, err: ProstTwirpError) -> ProstTwirpError {
        ProstTwirpError::AfterBodyError {
            body: self.output.clone(), method: None, version: self.version,
            headers: self.headers.clone(), status: Some(self.status), err: Box::new(err)
        }
    }

    pub fn to_proto<T: Message + Default + 'static>(&self) -> Result<ServiceResponse<T>, ProstTwirpError> {
        if self.status.is_success() {
            match T::decode(&self.output) {
                Ok(v) => Ok(self.clone_with_output(v)),
                Err(err) => Err(self.body_err(ProstTwirpError::ProstDecodeError(err)))
            }
        } else {
            match TwirpError::from_json_bytes(&self.output) {
                Ok(err) => Err(self.body_err(ProstTwirpError::TwirpError(err))),
                Err(err) => Err(self.body_err(ProstTwirpError::JsonDecodeError(err)))
            }
        }
    }
}

impl<T: Message + Default + 'static> ServiceResponse<T> {
    pub fn to_proto_raw(&self) -> Result<ServiceResponse<Vec<u8>>, ProstTwirpError> {
        let mut body = Vec::new();
        if let Err(err) = self.output.encode(&mut body) {
            Err(ProstTwirpError::ProstEncodeError(err))
        } else {
            Ok(self.clone_with_output(body))
        }
    }

    pub fn from_hyper_proto(resp: Response) -> FutResp<T> {
        Box::new(ServiceResponse::from_hyper_raw(resp).and_then(|v| v.to_proto()))
    }

    pub fn to_hyper_proto(&self) -> Result<Response, ProstTwirpError> {
        self.to_proto_raw().map(|v| v.to_hyper_raw())
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

    pub fn to_resp_raw(&self, status: StatusCode) -> ServiceResponse<Vec<u8>> {
        let output = self.to_json_bytes().unwrap_or_else(|_| "{}".as_bytes().to_vec());
        let mut headers = Headers::new();
        headers.set(ContentType::json());
        headers.set(ContentLength(output.len() as u64));
        ServiceResponse {
            version: HttpVersion::default(),
            headers: headers,
            status: status,
            output
        }
    }

    pub fn to_hyper_resp(&self, status: StatusCode) -> Response {
        let body = self.to_json_bytes().unwrap_or_else(|_| "{}".as_bytes().to_vec());
        Response::new().
            with_status(status).
            with_header(ContentType::json()).
            with_header(ContentLength(body.len() as u64)).
            with_body(body)
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

    pub fn go<I, O>(&self, path: &str, req: ServiceRequest<I>) -> FutResp<O>
            where I: Message + Default + 'static, O: Message + Default + 'static {
        // Build the URI
        let uri = match format!("{}/{}", self.root_url, path.trim_left_matches('/')).parse() {
            Err(err) => return Box::new(future::err(ProstTwirpError::HyperError(hyper::Error::Uri(err)))),
            Ok(v) => v,
        };
        // Build the request
        let mut hyper_req = match req.to_hyper_proto() {
            Err(err) => return Box::new(future::err(err)),
            Ok(v) => v
        };
        hyper_req.set_uri(uri);

        // Run the request and map the response
        Box::new(self.client.request(hyper_req).
            map_err(ProstTwirpError::HyperError).
            and_then(ServiceResponse::from_hyper_proto))
    }
}

pub trait HyperService {
    // Ug: https://github.com/tokio-rs/tokio-service/issues/9
    fn static_self(&self) -> Box<'static + HyperService>;

    fn handle(&self, req: ServiceRequest<Vec<u8>>) -> FutResp<Vec<u8>>;
}

pub struct HyperServer<T> {
    pub service: T
}
impl<T> HyperServer<T> {
    pub fn new(service: T) -> HyperServer<T> { HyperServer { service } }
}

impl<T: 'static + HyperService> Service for HyperServer<T> {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        if req.method() != &Method::Post {
            Box::new(future::ok(TwirpError::new("bad_method", "Must be 'POST'").
                to_hyper_resp(StatusCode::MethodNotAllowed)))
        } else {
            // Ug: https://github.com/tokio-rs/tokio-service/issues/9
            let static_self = self.service.static_self();
            Box::new(ServiceRequest::from_hyper_raw(req).
                and_then(move |v| static_self.handle(v)).
                map(|v| v.to_hyper_raw()).
                or_else(|err| {
                    let (status, twirp_err) = match err.root_err() {
                        // TODO
                        _ => (StatusCode::InternalServerError, TwirpError::new("internal_err", "Internal Error"))
                    };
                    Ok(twirp_err.to_hyper_resp(status))
                }))
        }
    }
}