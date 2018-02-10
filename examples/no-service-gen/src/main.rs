extern crate futures;
extern crate hyper;
extern crate prost;
#[macro_use]
extern crate prost_derive;
extern crate prost_twirp;
extern crate tokio_core;

use futures::Future;
use futures::future;
use hyper::Client;
use prost_twirp::HyperClient;
use std::env;
use tokio_core::reactor::Core;

mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}

fn main() {
    let mut core = Core::new().unwrap();
    let run_server = env::args().any(|s| s == "--server");
    let run_client = !run_server || env::args().any(|s| s == "--client");

    // TODO: run server

    if run_client {
        let hyper_client = Client::new(&core.handle());
        let prost_client = HyperClient::new(hyper_client, "http://localhost:8080");
        // Run the 5 like the other client
        let work = future::join_all((0..5).map(|_|
            prost_client.
                go("/twirp/twitch.twirp.example.Haberdasher/MakeHat", service::Size { inches: 12 }.into()).
                and_then(|res| {
                    let hat: service::Hat = res.output;
                    Ok(println!("Made {:?}", hat))
                })
        ));
        core.run(work).unwrap();
    }
}