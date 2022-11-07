# Prost Twirp

Prost Twirp is a code generator and set of utilities for calling and serving [Twirp](https://github.com/twitchtv/twirp)
services in Rust, using the [prost](https://github.com/danburkert/prost/) and [hyper](https://github.com/hyperium/hyper)
libraries.

See usage detail below, [API docs](https://docs.rs/prost-twirp), and [examples](examples).

## Usage

Prost Twirp supports the calling and the serving of Twirp services. Prost Twirp can be used in one of three ways:

* As a client and/or server code generator along with a supporting runtime library
* As a client and/or server code generator with supporting runtime needs embedded in the generated code
* As a library of utilities to help with more manual Twirp client/server invocations

Below will walkthrough code creation and service consumption/implementation.

### Generating Code

Most of the code generation relies on [prost](https://github.com/danburkert/prost/). The `prost` code generator accepts
a [ServiceGenerator](https://docs.rs/prost-build/0.3/prost_build/trait.ServiceGenerator.html). Prost Twirp provides this
generator. This walkthrough will use Twirp's [example service.proto](examples/service.proto) that is also used by the
Prost Twirp's [examples](examples).

Setup the project to generate code like the [prost-build](https://docs.rs/prost-build/) docs suggest. In addition, add
the following to the dependencies and build dependencies of `Cargo.toml`:

```toml
[dependencies]
prost-twirp = "prost-twirp-version"
futures = "futures-version"
hyper = "hyper-version"
tokio-core = "tokio-core-version"

[build-dependencies]
prost-twirp = { version = "prost-twirp-version", features = ["service-gen"] }
```

This adds the supporting Prost Twirp library at runtime and the service generation support at build time. It also adds
[hyper](https://hyper.rs/), [futures](https://docs.rs/futures), and [tokio](https://tokio.rs) that are needed to use the
service at runtime. Previously, the build script code in `build.rs` might have been:

```rust
fn main() {
    prost_build::compile_protos(&["src/service.proto"], &["src/"]).unwrap();
}
```

That would just generate the protobuf structs, but not the service. Now change it to utilize the Prost Twirp service
generator:

```rust
fn main() {
    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(prost_twirp::TwirpServiceGenerator::new()));
    conf.compile_protos(&["src/service.proto"], &["src/"]).unwrap();
}
```

Now the included file contains a service client and server. As in the `prost-build` docs, it can be included in
`main.rs`:

```rust
mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}
```

### Generated Trait

Each protobuf service is generated as a simple trait. The [example service.proto](examples/service.proto) contains the
following service:

```proto
// A Haberdasher makes hats for clients.
service Haberdasher {
  // MakeHat produces a hat of mysterious, randomly-selected color!
  rpc MakeHat(Size) returns (Hat);
}
```

This generates the following trait:

```rust
/// A Haberdasher makes hats for clients.
pub trait Haberdasher {
    /// MakeHat produces a hat of mysterious, randomly-selected color!
    fn make_hat(&self, i: PTReq<Size>) -> PTRes<Hat>;
}
```

[PTReq](https://docs.rs/prost-twirp/*/prost_twirp/type.PTReq.html) is just a `ServiceRequest`.
[PTRes](https://docs.rs/prost-twirp/*/prost_twirp/type.PTRes.html) is
`Box<Future<Item = ServiceResponse<O>, Error = ProstTwirpError>>`. This trait is used by both the client and the server.

### Using the Client

Creating a Prost Twirp client is just an extra step after
[creating the hyper::Client](https://hyper.rs/guides/client/basic/). Simply call `ServiceName::new_client` with the
hyper client and a root URL like so:

```rust
let hyper_client = Client::new(&core.handle());
let service_client = service::Haberdasher::new_client(hyper_client, "http://localhost:8080");
```

This creates and returns a boxed implementation of the client trait. Then it can be called like so:

```rust
let work = service_client.make_hat(service::Size { inches: 12 }.into()).
    and_then(|res| Ok(println!("Made {:?}", res.output)));
core.run(work).unwrap();
```

Notice the `into`, that turns a `prost` protobuf object into a Prost Twirp
[ServiceRequest](https://docs.rs/prost-twirp/*/prost_twirp/struct.ServiceRequest.html). The result is a boxed
future of the [ServiceResponse](https://docs.rs/prost-twirp/*/prost_twirp/struct.ServiceResponse.html) whose `output`
field will contain the serialized result (in this case, `service::Hat`).

Any error that can happen during the call results in an errored future with the
[ProstTwirpError](https://docs.rs/prost-twirp/*/prost_twirp/enum.ProstTwirpError.html) error.

### Using the Server

The same trait that is used for the client is what must be implemented as a server. Here is an example implementation:

```rust
pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(&self, i: service::PTReq<service::Size>) -> service::PTRes<service::Hat> {
        Box::new(future::ok(
            service::Hat { size: i.input.inches, color: "blue".to_string(), name: "fedora".to_string() }.into()
        ))
    }
}
```

Like other hyper services, this one returns a boxed future with the protobuf value. In this case, it just generates an
instance of `Hat` every time. Errors can be returned which are in the form of a
[ProstTwirpError](https://docs.rs/prost-twirp/*/prost_twirp/enum.ProstTwirpError.html). A
[TwirpError](https://docs.rs/prost-twirp/*/prost_twirp/struct.TwirpError.html) can be sent back instead. Here is an
example of not accepting any size outside of some bounds:

```rust
pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(&self, i: service::PTReq<service::Size>) -> service::PTRes<service::Hat> {
        Box::new(future::result(
            if i.input.inches < 1 {
                Err(TwirpError::new(StatusCode::BadRequest, "too_small", "Size too small")
            } else if i.input.inches > 10 {
                Err(TwirpError::new(StatusCode::BadRequest, "too_large", "Size too large")
            } else {
                Ok(service::Hat { size: i.input.inches, color: "blue".to_string(), name: "fedora".to_string() }.into())
            }
        ))
    }
}
```

Metadata in the form of a `serde_json::Value` can be given to a `TwirpError` as well. To start the service, there is a
`ServiceName::new_server` call that accepts an implementation of the trait and returns a `hyper::server::Service` that
can be [used like any other hyper service](https://hyper.rs/guides/server/hello-world/). E.g.

```rust
let addr = "0.0.0.0:8080".parse().unwrap();
let server = Http::new().bind(&addr,
    move || Ok(service::Haberdasher::new_server(HaberdasherService))).unwrap();
server.run().unwrap();
```

Note, due to [some tokio service restrictions](https://github.com/tokio-rs/tokio-service/issues/9), the service
implementation has to have a `'static` lifetime.

### Embedding the Runtime

Instead of having a runtime dependency on the `prost_twirp` crate, it can be embedded instead. By creating the
`TwirpServiceGenerator` as a mut variable and setting `embed_client` to true, the entire runtime code (not that big)
will be put in a `prost_twirp` nested module and referenced in the generated code. This means that `prost-twirp` doesn't
have to be set in the `[dependencies]` for runtime. However, besides `prost` and `prost-derive` runtime libraries,
Prost Twirp does still require `serde_json` at runtime for error serialization.

### Manual Client and Server

Instead of code generation, some of the features of Prost Twirp can be used manually.

For the client, a new [HyperClient](https://docs.rs/prost-twirp/*/prost_twirp/struct.HyperClient.html) can be created
with the root URL and `hyper` client. Then, `go` can be invoked with a path and
a [ServiceRequest](https://docs.rs/prost-twirp/*/prost_twirp/struct.ServiceRequest.html) for a `prost`-built message.
The response is a boxed future of a
[ServiceResponse](https://docs.rs/prost-twirp/*/prost_twirp/struct.ServiceResponse.html) that must be typed with the
expected `prost`-built output type. Example:

```rust
let prost_client = HyperClient::new(hyper_client, "http://localhost:8080");
let work = prost_client.
    go("/twirp/twitch.twirp.example.Haberdasher/MakeHat",
        ServiceRequest::new(service::Size { inches: 12 })).
    and_then(|res| {
        let hat: service::Hat = res.output;
        Ok(println!("Made {:?}", hat))
    });
```

For the server, a new [HyperServer](https://docs.rs/prost-twirp/*/prost_twirp/struct.HyperServer.html) can be created
passing in an impl of [HyperService](https://docs.rs/prost-twirp/*/prost_twirp/trait.HyperService.html). The
`HyperService` trait is essentially just a handler for accepting a `ServiceRequest<Vec<u8>>` and returning a boxed
future of `ServiceResponse<Vec<u8>>`. Inside the handler, `prost`-built structs can be serialized/deserialized.

### FAQ

**Why no JSON support?**

This could be done soon. I am investigating whether this is as easy as a couple of `serde` attributes or if it is more
involved.

**Why does my server service impl have to be `'static`?**

This is due to the need to reference the service inside of static futures. See
[this issue](https://github.com/tokio-rs/tokio-service/issues/9). Any better solution is welcome.
