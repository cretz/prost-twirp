extern crate futures;
extern crate hyper;
extern crate prost;
#[macro_use]
extern crate prost_derive;
extern crate prost_twirp;
extern crate tokio_core;

use futures::Future;
use hyper::Client;
use prost_twirp::HyperClient;
use tokio_core::reactor::Core;

mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}

fn main() {
    let mut core = Core::new().unwrap();
    let hyper_client = Client::new(&core.handle());
    let prost_client = HyperClient::new(hyper_client, "http://localhost:8080");
    let hat_size = service::Size { inches: 8 };
    let work = prost_client.go("/twirp/twirp.example.haberdasher.Haberdasher/MakeHat", hat_size).and_then(|res| {
        let hat: service::Hat = res.rpc_response;
        println!("HAT: {:?}", hat);
        Ok(())
    });
    core.run(work).unwrap();
}