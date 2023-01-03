# Prost Twirp

Prost Twirp is a code generator and set of utilities for calling and serving [Twirp](https://github.com/twitchtv/twirp)
services in Rust, using the [prost](https://github.com/danburkert/prost/) and [hyper](https://github.com/hyperium/hyper)
libraries.

Twirp is a simple cross-language framework/protocol for RPC, with services defined in Protobuf and transmitted by HTTP POST.

See usage detail below, [API docs](https://docs.rs/prost-twirp), and [examples](https://github.com/sourcefrog/prost-twirp/tree/master/examples).

## Usage

Prost Twirp supports the calling and the serving of Twirp services. Prost Twirp can be used in one of three ways, each
explained in the following sections.

Because of the dynamically generated code and the interactions with complex Hyper types, the 
best way to understand the API is to read and experiment with the `examples/`, in
particular `examples/service-gen`, which is the simplest.

* As a client and/or server code generator along with a supporting runtime library. This is the simplest approach and strongly recommended.
* As a client and/or server code generator with supporting runtime needs embedded in the generated code
* As a library of utilities to help with more manual Twirp client/server invocations

### Generating Code

See `examples/service-gen` for a full working example.

Most of the code generation relies on [prost](https://github.com/danburkert/prost/). The `prost` code generator accepts
a [prost_build::ServiceGenerator]. Prost Twirp provides this
generator. 

This walkthrough will use Twirp's [example service.proto](https://twitchtv.github.io/twirp/docs/example.html) that is also used by the
Prost Twirp's examples.

Setup the project to generate code like the [prost-build](https://docs.rs/prost-build/) docs suggest. In addition, add
the following to the dependencies and build dependencies of `Cargo.toml`:

```toml
[dependencies]
bytes = "1.2"
futures = "0.3"
prost = "0.11"
prost-derive = "0.11"
prost-twirp = "0.2"

[dependencies.hyper]
version = "0.14"
features = ["client", "server", "http1", "http2", "tcp"]

[dependencies.tokio]
version = "1.2"
features = ["macros", "net", "rt", "rt-multi-thread", "sync", "time"]

[build-dependencies]
prost-build = "0.11"
prost-twirp = { features = ["service-gen"] }
```

This adds the supporting Prost Twirp library at runtime and the service generation support at build time. It also adds
[hyper](https://hyper.rs/), [futures](https://docs.rs/futures), and [tokio](https://tokio.rs) that are needed to use the
service at runtime. Previously, the build script code in `build.rs` might have been:

```rust,ignore
fn main() {
    prost_build::compile_protos(&["src/service.proto"], &["src/"]).unwrap();
}
```

That would just generate the protobuf structs, but not the service. Now change it to utilize the Prost Twirp service
generator:

```rust,ignore
fn main() {
    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(prost_twirp::TwirpServiceGenerator::new()));
    conf.compile_protos(&["src/service.proto"], &["src/"]).unwrap();
}
```

Now the included file contains a service client and server. As in the `prost-build` docs, it can be included in
`main.rs`:

```rust,ignore
mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}
```

### Generated Trait

Each protobuf service maps to a single auto-generated Rust trait, with one method 
corresponding to each method in the proto service interface. 

The trait is implemented by an auto-generated client stub type, which translates 
method calls into HTTP requests to a remote Twirp server. 

If you write a server, then your server will also provide an implementation 
of the same service trait, which when the methods are called will execute the business
logic of the method: for example, making a hat.

The [example service.proto](examples/service.proto) contains the
following service:

```proto
// A Haberdasher makes hats for clients.
service Haberdasher {
  // MakeHat produces a hat of mysterious, randomly-selected color!
  rpc MakeHat(Size) returns (Hat);
}
```

This generates the following trait in `target/..../twitch.twirp.example.rs`:

```rust,ignore
pub trait Haberdasher: Send + Sync + 'static {
    /// MakeHat produces a hat of mysterious, randomly-selected color!
    fn make_hat(
        &self,
        request: ::prost_twirp::ServiceRequest<Size>,
    ) -> ::prost_twirp::PTRes<Hat>;
}

impl dyn Haberdasher {
    pub fn new_client(
        client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>,
        root_url: &str,
    ) -> Box<dyn Haberdasher> {
        /* ... */
    }

    pub fn new_server<T: Haberdasher>(
        v: T,
    ) -> Box<
        dyn (::hyper::service::Service<
            ::hyper::Request<::hyper::Body>,
            Response = ::hyper::Response<::hyper::Body>,
            Error = ::hyper::Error,
            Future = ::std::pin::Pin<
                Box<
                    dyn (::futures::Future<
                        Output = Result<::hyper::Response<::hyper::Body>, ::hyper::Error>,
                    >) + Send,
                >,
            >,
        >) + Send + Sync,
    > {
        /* ... */
    }
}

```

[PTRes] is a boxed future service response, used by both the client and the server.

### Using the Client

Creating a Prost Twirp client is just an extra step after creating the
[hyper::Client]. Simply call the `new_client` static method of the generated
service trait, passing the hyper client and a root URL like so:

```rust,ignore
let hyper_client = Client::new();
let service_client =
    <dyn service::Haberdasher>::new_client(hyper_client, "http://localhost:8080");
```

This creates and returns a boxed implementation of the `Haberdasher` trait. Then it can be called like so:

```rust,ignore
let res = service_client
    .make_hat(service::Size { inches: 12 }.into())
    .await
    .unwrap();
println!("Made {:?}", res.output);
```

Notice the `into`, that turns a `prost` protobuf object into a Prost Twirp
[ServiceRequest]. The result is a boxed
future of the [ServiceResponse] whose `output`
field will contain the serialized result (in this case, `service::Hat`).

Any error that can happen during the call results in an errored future with the
[ProstTwirpError] error.

### Using the Server

The same trait that is used for the client is what must be implemented as a server. Here is an example implementation:

```rust,ignore
pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(
        &self,
        req: service::ServiceRequest<service::Size>,
    ) -> service::PTRes<service::Hat> {
        Box::pin(future::ok(
            service::Hat {
                size: req.input.inches,
                color: "blue".to_string(),
                name: "fedora".to_string(),
            }
            .into(),
        ))
    }
}
```

Like other Hyper services, this returns a boxed future with the protobuf value. In this case, it just generates an
instance of `Hat` every time. 

To start the service, the generated trait has a
`new_server` method that accepts an implementation of the trait and returns a `::hyper::service::Service`.

```rust,ignore
let addr = "0.0.0.0:8080".parse().unwrap();
let server = Server::bind(&addr)
    .serve(make_service_fn(|_conn| async {
        Ok::<_, Infallible>(<dyn service::Haberdasher>::new_server(HaberdasherService))
    }));
server.await.unwrap();
```

Note, due to [some tokio service restrictions](https://github.com/tokio-rs/tokio-service/issues/9), the service
implementation has to have a `'static` lifetime.

## Returning Errors

Errors can be returned which are in the form of a [ProstTwirpError]. A [TwirpError], which corresponds more directly
to the Twirp serialized error format can be sent back instead. 

(See `examples/errors` for a full working example.)

Here is an example of not accepting any size outside of some bounds:

```rust,ignore
pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(&self, i: service::ServiceRequest<service::Size>) -> service::PTRes<service::Hat> {
        Box::pin(if i.input.inches < 1 {
            future::err(
                TwirpError::new_meta(
                    StatusCode::BAD_REQUEST,
                    "too_small",
                    "Size too small",
                    serde_json::to_value(MinMaxSize { min: 1, max: 10 }).ok(),
                )
                .into(),
            )
        } else if i.input.inches > 10 {
            future::err(
                TwirpError::new_meta(
                    StatusCode::BAD_REQUEST,
                    "too_large",
                    "Size too large",
                    serde_json::to_value(MinMaxSize { min: 1, max: 10 }).ok(),
                )
                .into(),
            )
        } else {
            future::ok(
                service::Hat {
                    size: i.input.inches,
                    color: "blue".to_string(),
                    name: "fedora".to_string(),
                }
                .into(),
            )
        })
    }
}
```

Metadata in the form of a [serde_json::Value] can be given to a [TwirpError] as well. 

### Embedding the Runtime

Instead of having a runtime dependency on the `prost_twirp` crate, it can be embedded instead. By creating the
`TwirpServiceGenerator` as a mut variable and setting `embed_client` to true, the entire runtime code (not that big)
will be put in a `prost_twirp` nested module and referenced in the generated code. This means that `prost-twirp` doesn't
have to be set in the `[dependencies]` for runtime. However, besides `prost` and `prost-derive` runtime libraries,
Prost Twirp does still require `serde_json` at runtime for error serialization.

### Manual Client and Server

Instead of code generation, some of the features of Prost Twirp can be used manually.
See `examples/no-service-gen`. In this mode the application code is responsible for URL
routing and determining the right request and response type, and `prost_twirp` will
de/serialize requests and responses.
 
For the client, a new [HyperClient] can be created
with the root URL and `hyper` client. Then, `go` can be invoked with a path and
a [ServiceRequest] for a `prost`-built message.
The response is a boxed future of a
[ServiceResponse] that must be typed with the
expected `prost`-built output type. Example:

### FAQ

#### Why no JSON support?

This could be done soon, probably using [`pbjson`](https://docs.rs/pbjson/).

#### Why does my server service impl have to be `'static`?

This is due to the need to reference the service inside of static futures. See
[this issue](https://github.com/tokio-rs/tokio-service/issues/9). Any better solution is welcome.

#### What Twirp format is supported?

This crate currently implements Twirp 5. Twirp 7 could be added.

