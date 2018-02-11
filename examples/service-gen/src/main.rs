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
use hyper::Client;
use hyper::server::Http;
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
            let server = Http::new().bind(&addr,
                move || Ok(service::Haberdasher::new_server(HaberdasherService))).unwrap();
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
        let service_client = service::Haberdasher::new_client(hyper_client, "http://localhost:8080");
        // Run the 5 like the other client
        let work = future::join_all((0..5).map(|_|
            service_client.make_hat(service::Size { inches: 12 }.into()).
                and_then(|res| Ok(println!("Made {:?}", res.output)))
        ));
        core.run(work).unwrap();
        shutdown_send.send(()).unwrap();
    }
}

pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(&self, i: service::PTReq<service::Size>) -> service::PTRes<service::Hat> {
        Box::new(future::ok(
            service::Hat { size: i.input.inches, color: "blue".to_string(), name: "fedora".to_string() }.into()
        ))
    }
}
