extern crate futures;
extern crate hyper;
extern crate prost;
#[macro_use]
extern crate prost_derive;
extern crate prost_twirp;
extern crate tokio_core;

use futures::Future;
use futures::future;
use futures::sync::oneshot;
use hyper::{Client, Method, StatusCode};
use hyper::server::Http;
use prost_twirp::{FutResp, HyperClient, HyperServer, HyperService, ServiceRequest, ServiceResponse, TwirpError};
use std::env;
use std::thread;
use std::time::Duration;
use tokio_core::reactor::Core;

mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}

fn main() {
    let run_server = env::args().any(|s| s == "--server");
    let run_client = !run_server || env::args().any(|s| s == "--client");
    let (shutdown_send, shutdown_recv) = oneshot::channel();

    if run_server {
        let thread_res = thread::spawn(|| {
            println!("Starting server");
            let addr = "0.0.0.0:8080".parse().unwrap();
            let server = Http::new().bind(&addr, move || Ok(HyperServer::new(MyServer))).unwrap();
            server.run_until(shutdown_recv.map_err(|_| ())).unwrap();
            println!("Server stopped");
        });
        // Wait a sec or forever depending on whether there's client code to run
        if run_client {
            thread::sleep(Duration::from_millis(1000));
        } else {
            if let Err(err) = thread_res.join() { println!("Server panicked: {:?}", err); }
        }
    }

    if run_client {
        let mut core = Core::new().unwrap();
        let hyper_client = Client::new(&core.handle());
        let prost_client = HyperClient::new(hyper_client, "http://localhost:8080");
        // Run the 5 like the other client
        let work = future::join_all((0..5).map(|_|
            prost_client.
                go("/twirp/twitch.twirp.example.Haberdasher/MakeHat",
                    ServiceRequest::new(service::Size { inches: 12 })).
                and_then(|res| {
                    let hat: service::Hat = res.output;
                    Ok(println!("Made {:?}", hat))
                })
        ));
        core.run(work).unwrap();
        shutdown_send.send(()).unwrap();
    }
}

struct MyServer;
impl HyperService for MyServer {
    fn handle(&self, req: ServiceRequest<Vec<u8>>) -> FutResp<Vec<u8>> {
        match (req.method.clone(), req.uri.path()) {
            (Method::Post, "/twirp/twitch.twirp.example.Haberdasher/MakeHat") =>
                Box::new(future::result(req.to_proto().and_then(|req| {
                    let size: service::Size = req.input;
                    ServiceResponse::new(
                        service::Hat { size: size.inches, color: "blue".to_string(), name: "fedora".to_string() }
                    ).to_proto_raw()
                }))),
            _ => Box::new(future::ok(TwirpError::new("not_found", "Not found").to_resp_raw(StatusCode::NotFound)))
        }
    }
}
