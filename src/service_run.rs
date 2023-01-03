use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::future::ready;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{future, Future, TryFutureExt};
use hyper::client::HttpConnector;
use hyper::header::{HeaderMap, ALLOW, CONTENT_LENGTH, CONTENT_TYPE};
use hyper::http::{self, HeaderValue};
use hyper::service::Service;
use hyper::{Body, Client, Method, Request, Response, StatusCode, Uri, Version};
use prost::{DecodeError, EncodeError, Message};

/// The type of every service response
pub type PTRes<O> =
    Pin<Box<dyn Future<Output = Result<ServiceResponse<O>, ProstTwirpError>> + Send + 'static>>;

static JSON_CONTENT_TYPE: &str = "application/json";
static PROTOBUF_CONTENT_TYPE: &str = "application/protobuf";

/// A request with HTTP info and a proto request payload object.
#[derive(Debug)]
pub struct ServiceRequest<T: Message> {
    /// The URI of the original request
    ///
    /// When using a client, this will be overridden with the proper URI. It is only valuable for servers.
    pub uri: Uri,
    /// The request method; should always be `POST`.
    pub method: Method,
    /// The HTTP version, rarely changed from the default.
    pub version: Version,
    /// The set of headers
    ///
    /// Should always at least have `Content-Type`. Clients will override `Content-Length` on serialization.
    pub headers: HeaderMap,
    /// The request body as a proto `Message`, representing the arguments of the proto rpc.
    pub input: T,
}

impl<T: Message> ServiceRequest<T> {
    /// Create new service request with the given input object
    ///
    /// This automatically sets the `Content-Type` header as `application/protobuf`.
    pub fn new(input: T) -> ServiceRequest<T> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static(PROTOBUF_CONTENT_TYPE),
        );
        ServiceRequest {
            uri: Default::default(),
            method: Method::POST,
            version: Version::default(),
            headers,
            input,
        }
    }

    /// Copy this request with a different input value
    pub fn clone_with_input(&self, input: T) -> ServiceRequest<T> {
        ServiceRequest {
            uri: self.uri.clone(),
            method: self.method.clone(),
            version: self.version,
            headers: self.headers.clone(),
            input,
        }
    }
}

impl<T: Message + Default + 'static> From<T> for ServiceRequest<T> {
    fn from(v: T) -> ServiceRequest<T> {
        ServiceRequest::new(v)
    }
}

impl<T: Message + Default + 'static> ServiceRequest<T> {
    /// Serialize into a hyper request.
    pub fn to_hyper_request(&self) -> Result<Request<Body>, ProstTwirpError> {
        let mut body = Vec::new();
        self.input
            .encode(&mut body)
            .map_err(ProstTwirpError::ProstEncodeError)?;
        let mut builder = Request::post(self.uri.clone());
        builder.headers_mut().unwrap().clone_from(&self.headers);
        builder
            .header(CONTENT_LENGTH, body.len() as u64)
            .body(Body::from(body))
            .map_err(ProstTwirpError::from)
    }

    pub async fn from_hyper_request(
        req: Request<Body>,
    ) -> Result<ServiceRequest<T>, ProstTwirpError> {
        if req.method() != Method::POST {
            return Err(ProstTwirpError::InvalidMethod);
        } else if req
            .headers()
            .get(CONTENT_TYPE)
            .map_or(true, |v| v != PROTOBUF_CONTENT_TYPE)
        {
            return Err(ProstTwirpError::InvalidContentType);
        }
        let uri = req.uri().clone();
        let method = req.method().clone();
        let version = req.version();
        let headers = req.headers().clone();
        let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
        match T::decode(body_bytes.clone()) {
            Ok(input) => Ok(ServiceRequest {
                uri,
                method,
                version,
                headers,
                input,
            }),
            Err(err) => Err(ProstTwirpError::AfterBodyError {
                status: None,
                method: Some(method),
                version,
                headers,
                err: Box::new(ProstTwirpError::ProstDecodeError(err)),
                body: body_bytes.to_vec(),
            }),
        }
    }
}

/// A response with HTTP info and the output object as a protobuf [Message].
#[derive(Debug)]
pub struct ServiceResponse<M: Message> {
    /// The HTTP version
    pub version: Version,
    /// The set of headers
    ///
    /// Should always at least have `Content-Type`. Servers will override `Content-Length` on serialization.
    pub headers: HeaderMap,
    /// The status code
    pub status: StatusCode,
    /// The output object
    pub output: M,
}

impl<M: Message> ServiceResponse<M> {
    /// Create new service request with the given input object
    ///
    /// This automatically sets the `Content-Type` header as `application/protobuf`.
    pub fn new(output: M) -> ServiceResponse<M> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static(PROTOBUF_CONTENT_TYPE),
        );
        ServiceResponse {
            version: Version::default(),
            headers,
            status: StatusCode::OK,
            output,
        }
    }

    /// Copy this response with a different output value
    pub fn clone_with_output(&self, output: M) -> ServiceResponse<M> {
        ServiceResponse {
            version: self.version,
            headers: self.headers.clone(),
            status: self.status,
            output,
        }
    }
}

impl<M: Message + Default + 'static> From<M> for ServiceResponse<M> {
    fn from(v: M) -> ServiceResponse<M> {
        ServiceResponse::new(v)
    }
}

impl<M: Message + Default> ServiceResponse<M> {
    /// Deserialze an object response from a hyper response.
    pub async fn from_hyper_response(resp: Response<Body>) -> Result<Self, ProstTwirpError> {
        let version = resp.version();
        let headers = resp.headers().clone();
        let status = resp.status();
        let body_bytes = hyper::body::to_bytes(resp.into_body()).await?;
        let err = if status.is_success() {
            match M::decode(&*body_bytes) {
                Ok(output) => {
                    return Ok(ServiceResponse {
                        version,
                        headers,
                        status,
                        output,
                    })
                }
                Err(err) => ProstTwirpError::ProstDecodeError(err),
            }
        } else {
            match TwirpError::from_json_bytes(status, &body_bytes) {
                Ok(err) => ProstTwirpError::TwirpError(err),
                Err(err) => ProstTwirpError::JsonDecodeError(err),
            }
        };
        Err(ProstTwirpError::AfterBodyError {
            body: body_bytes.to_vec(),
            method: None,
            version,
            headers,
            status: Some(status),
            err: Box::new(err),
        })
    }

    /// Serialize an object response into a hyper response.
    pub fn to_hyper_response(&self) -> Result<Response<Body>, ProstTwirpError> {
        let body_bytes = self.output.encode_to_vec();
        let mut builder = Response::builder().status(self.status);
        builder.headers_mut().unwrap().clone_from(&self.headers);
        builder
            .header(CONTENT_LENGTH, body_bytes.len() as u64)
            .body(body_bytes.into())
            .map_err(ProstTwirpError::from)
    }
}

/// A JSON-serializable Twirp error
#[derive(Debug, Clone)]
pub struct TwirpError {
    pub status: StatusCode,
    pub error_type: String,
    pub msg: String,
    pub meta: Option<serde_json::Value>,
}

impl TwirpError {
    /// Create a Twirp error with no meta
    pub fn new(status: StatusCode, error_type: &str, msg: &str) -> TwirpError {
        TwirpError::new_meta(status, error_type, msg, None)
    }

    /// Create a Twirp error with optional meta
    pub fn new_meta(
        status: StatusCode,
        error_type: &str,
        msg: &str,
        meta: Option<serde_json::Value>,
    ) -> TwirpError {
        TwirpError {
            status,
            error_type: error_type.to_string(),
            msg: msg.to_string(),
            meta,
        }
    }

    /// Create a hyper response for this error and the given status code
    pub fn to_hyper_response(&self) -> Response<Body> {
        let body_bytes = self
            .to_json_bytes()
            .unwrap_or_else(|_| "{}".as_bytes().to_vec());
        let body_len = body_bytes.len() as u64;
        Response::builder()
            .status(self.status)
            .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            .header(CONTENT_LENGTH, HeaderValue::from(body_len))
            .header(ALLOW, HeaderValue::from_static("POST"))
            .body(Body::from(body_bytes))
            .expect("failed to serialize twirp error")
        // The potential panic here is not desirable but it seems highly
        // unlikely that we fail to serialize a body from a simple string
        // like this.
    }

    /// Create error from Serde JSON value
    pub fn from_json(status: StatusCode, json: serde_json::Value) -> TwirpError {
        let error_type = json["error_type"].as_str();
        TwirpError {
            status,
            error_type: error_type.unwrap_or("<no code>").to_string(),
            msg: json["msg"].as_str().unwrap_or("<no message>").to_string(),
            // Put the whole thing as meta if there was no type
            meta: if error_type.is_some() {
                json.get("meta").cloned()
            } else {
                Some(json.clone())
            },
        }
    }

    /// Create error from byte array
    pub fn from_json_bytes(status: StatusCode, json: &[u8]) -> serde_json::Result<TwirpError> {
        serde_json::from_slice(json).map(|v| TwirpError::from_json(status, v))
    }

    /// Create Serde JSON value from error
    pub fn to_json(&self) -> serde_json::Value {
        let mut props = serde_json::map::Map::new();
        props.insert(
            "error_type".to_string(),
            serde_json::Value::String(self.error_type.clone()),
        );
        props.insert(
            "msg".to_string(),
            serde_json::Value::String(self.msg.clone()),
        );
        if let Some(ref meta) = self.meta {
            props.insert("meta".to_string(), meta.clone());
        }
        serde_json::Value::Object(props)
    }

    /// Create byte array from error
    pub fn to_json_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&self.to_json())
    }
}

impl Error for TwirpError {}

impl Display for TwirpError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?} {}: {}", self.status, self.error_type, self.msg)
    }
}

impl From<TwirpError> for ProstTwirpError {
    fn from(v: TwirpError) -> ProstTwirpError {
        ProstTwirpError::TwirpError(v)
    }
}

/// An error that can occur during a call to a Twirp service
#[derive(Debug)]
#[non_exhaustive]
pub enum ProstTwirpError {
    /// A standard Twirp error with a type, message, and some metadata
    TwirpError(TwirpError),
    /// An error when trying to decode JSON into an error or object
    JsonDecodeError(serde_json::Error),
    /// An error when trying to encode a protobuf object
    ProstEncodeError(EncodeError),
    /// An error when trying to decode a protobuf object
    ProstDecodeError(DecodeError),
    /// A generic hyper error
    HyperError(hyper::Error),
    /// A HTTP protocol error
    HttpError(http::Error),
    /// An invalid URI.
    InvalidUri(http::uri::InvalidUri),
    /// The HTTP Method was not `POST`.
    InvalidMethod,
    /// The request content type was not `application/protobuf`.
    InvalidContentType,
    /// No matching method was found for the request.
    NotFound,
    /// A wrapper for any of the other `ProstTwirpError`s that also includes request/response info
    AfterBodyError {
        /// The request or response's raw body before the error happened
        body: Vec<u8>,
        /// The request method, only present for server errors
        method: Option<Method>,
        /// The request or response's HTTP version
        version: Version,
        /// The request or response's headers
        headers: HeaderMap,
        /// The response status, only present for client errors
        status: Option<StatusCode>,
        /// The underlying error
        err: Box<ProstTwirpError>,
    },
}

impl ProstTwirpError {
    /// This same error, or the underlying error if it is an `AfterBodyError`
    pub fn root_err(self) -> ProstTwirpError {
        match self {
            ProstTwirpError::AfterBodyError { err, .. } => err.root_err(),
            _ => self,
        }
    }

    pub fn into_hyper_response(self) -> Result<Response<Body>, hyper::Error> {
        let external_err = match self {
            ProstTwirpError::TwirpError(err) => err,
            // Just propagate hyper errors
            ProstTwirpError::HyperError(err) => return Err(err),
            ProstTwirpError::InvalidMethod => TwirpError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "bad_method",
                "Method must be POST",
            ),
            ProstTwirpError::ProstDecodeError(_) => TwirpError::new(
                StatusCode::BAD_REQUEST,
                "protobuf_decode_err",
                "Invalid protobuf body",
            ),
            ProstTwirpError::InvalidContentType => TwirpError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "bad_content_type",
                "Content type must be application/protobuf",
            ),
            ProstTwirpError::NotFound => TwirpError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "The requested method was not found",
            ),
            _ => TwirpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_err",
                "Internal error",
            ),
        };
        Ok(external_err.to_hyper_response())
    }
}

impl From<hyper::Error> for ProstTwirpError {
    fn from(v: hyper::Error) -> ProstTwirpError {
        ProstTwirpError::HyperError(v)
    }
}

impl From<http::Error> for ProstTwirpError {
    fn from(v: http::Error) -> ProstTwirpError {
        ProstTwirpError::HttpError(v)
    }
}

impl Display for ProstTwirpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for ProstTwirpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ProstTwirpError::TwirpError(err) => Some(err),
            ProstTwirpError::JsonDecodeError(err) => Some(err),
            ProstTwirpError::ProstEncodeError(err) => Some(err),
            ProstTwirpError::ProstDecodeError(err) => Some(err),
            ProstTwirpError::HyperError(err) => Some(err),
            ProstTwirpError::HttpError(err) => Some(err),
            ProstTwirpError::InvalidUri(err) => Some(err),
            ProstTwirpError::InvalidMethod => None,
            ProstTwirpError::InvalidContentType => None,
            ProstTwirpError::NotFound => None,
            ProstTwirpError::AfterBodyError { err, .. } => Some(err),
        }
    }
}

/// A wrapper for a hyper client
#[derive(Debug)]
pub struct HyperClient {
    /// The hyper client
    pub client: Client<HttpConnector>,
    /// The root URL without any path attached
    pub root_url: String,
}

impl HyperClient {
    /// Create a new client wrapper for the given client and root using protobuf
    pub fn new(client: Client<HttpConnector>, root_url: &str) -> HyperClient {
        HyperClient {
            client,
            root_url: root_url.trim_end_matches('/').to_string(),
        }
    }

    /// Invoke the given request for the given path and return a boxed future result
    pub fn go<I, O>(&self, path: &str, req: ServiceRequest<I>) -> PTRes<O>
    where
        I: Message + Default + 'static,
        O: Message + Default + 'static,
    {
        // Build the URI
        let uri = match format!("{}/{}", self.root_url, path.trim_start_matches('/')).parse() {
            Err(err) => return Box::pin(ready(Err(ProstTwirpError::InvalidUri(err)))),
            Ok(v) => v,
        };
        // Build the request
        let mut hyper_req = match req.to_hyper_request() {
            Err(err) => return Box::pin(ready(Err(err))),
            Ok(v) => v,
        };
        *hyper_req.uri_mut() = uri;
        // Run the request and map the response
        Box::pin(
            self.client
                .request(hyper_req)
                .map_err(ProstTwirpError::HyperError)
                .and_then(ServiceResponse::from_hyper_response),
        )
    }
}

/// A trait for the heart of a Twirp service: responding to every service method.
///
/// Implementations are responsible for:
///
/// 1. Matching the URL to a service method (or returning a 404).
/// 2. Decoding the request body into a protobuf message, typically
///    using [ServiceRequest::from_hyper_request] for the appropriate
///    message.
/// 3. Calling the application logic to handle the request.
/// 4. Encoding the response into a protobuf message, typically using
///    [ServiceResponse::to_hyper_response].
///
/// An implementation of this trait is generated by the `service-gen`
/// integration, or it can be implemented manually.
pub trait HyperService {
    /// Accept a raw service request and return a boxed future of a raw service response
    fn handle(
        &self,
        req: Request<Body>,
    ) -> Pin<Box<dyn Future<Output = Result<Response<Body>, ProstTwirpError>> + Send>>;
}

/// A wrapper for a [HyperService] trait that keeps a [Arc] version of the
/// service.
///
/// This layer checkcs preconditions of the request (the method and content
/// type) and translates any errors into the Twirp json format.
///
/// TODO: Perhaps a clearer name indicating this is a layer?
///
/// TODO: Perhaps change to a Tower `Layer`, although that would require
/// another dependency on `tower_layer`.
pub struct HyperServer<T: HyperService + Send + Sync + 'static> {
    /// The `Arc` version of the service
    ///
    /// Needed because of [hyper Service lifetimes](https://github.com/tokio-rs/tokio-service/issues/9)
    pub service: Arc<T>,
}

impl<T: HyperService + Send + Sync + 'static> HyperServer<T> {
    /// Create a new service wrapper for the given impl
    pub fn new(service: T) -> HyperServer<T> {
        HyperServer {
            service: Arc::new(service),
        }
    }
}

impl<T: 'static + HyperService + Send + Sync> Service<Request<Body>> for HyperServer<T> {
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn (Future<Output = Result<Self::Response, Self::Error>>) + Send>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // Ug: https://github.com/tokio-rs/tokio-service/issues/9
        let service = self.service.clone();
        Box::pin(
            service
                .handle(req)
                .or_else(|err| future::ready(err.into_hyper_response())),
        )
    }
}
