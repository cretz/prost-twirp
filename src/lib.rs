//! Prost Twirp is a code generator and set of utilities for calling and serving
//! [Twirp](https://github.com/twitchtv/twirp) services in Rust, using the [prost](https://github.com/danburkert/prost/)
//! and [hyper](https://github.com/hyperium/hyper) libraries.
//!
//! See [the github project](https://github.com/cretz/prost-twirp) for more info.

extern crate futures;
extern crate hyper;
extern crate prost;
extern crate serde_json;

mod service_run;
pub use service_run::*;

#[cfg(feature = "service-gen")]
extern crate prost_build;
#[cfg(feature = "service-gen")]
mod service_gen;
#[cfg(feature = "service-gen")]
pub use service_gen::TwirpServiceGenerator;
