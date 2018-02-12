# Prost Twirp

Prost Twirp is a code generator and set of utilities for calling and serving [Twirp](https://github.com/twitchtv/twirp)
services in Rust, using the [prost](https://github.com/danburkert/prost/) and [hyper](https://github.com/hyperium/hyper)
libraries.

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
prost-twirp = <prost-twirp-version>
futures = <futures-version>
hyper = <hyper-version>
tokio-core = <tokio-core-version>

[build-dependencies]
prost-twirp = { version = <prost-twirp-version>, features = ["service-gen"] }
```

This adds the supporting Prost Twirp library at runtime and the service generation support at build time. It also adds
[hyper](https://hyper.rs/), [futures](https://docs.rs/futures), and [tokio](https://tokio.rs) that are needed to use the
service at runtime. Previously, the build script code in `build.rs` might have been:

```rust
extern crate prost_build;

fn main() {
    prost_build::compile_protos(&["src/service.proto"], &["src/"]).unwrap();
}
```

That would just generate the protobuf structs, but not the service. Now change it to utilize the Prost Twirp service
generator:

```rust
extern crate prost_build;
extern crate prost_twirp;

fn main() {
    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(prost_twirp::TwirpServiceGenerator::new()));
    conf.compile_protos(&["src/service.proto"], &["src/"]).unwrap();
}
```

Now the included file contains a service client and server. As in the `prost-build` docs, it can be included in
`main.rs`:

```rust
extern crate futures;
extern crate hyper;
extern crate prost;
#[macro_use]
extern crate prost_derive;
extern crate prost_twirp;

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
    fn make_hat(&self, i: prost_twirp::PTReq<Size>) -> prost_twirp::PTRes<Hat>;
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

TODO

### JSON Support

TODO

### Embedding the Runtime

TODO

### Manual Client and Server

TODO