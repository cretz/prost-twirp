use futures::channel::oneshot;
use futures::future;
use hyper::server::Http;
use hyper::{Client, StatusCode};
use prost_twirp::TwirpError;
use serde_derive::{Deserialize, Serialize};
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
            let server = Http::new()
                .bind(&addr, move || {
                    Ok(<dyn service::Haberdasher>::new_server(HaberdasherService))
                })
                .unwrap();
            server.run_until(shutdown_recv.map_err(|_| ())).unwrap();
            println!("Server stopped");
        });
        // Wait a sec or forever depending on whether there's client code to run
        if run_client {
            thread::sleep(Duration::from_millis(1000));
        } else {
            if let Err(err) = thread_res.join() {
                println!("Server panicked: {:?}", err);
            }
        }
    }

    if run_client {
        let mut core = Core::new().unwrap();
        let hyper_client = Client::new(&core.handle());
        let service_client =
            <dyn service::Haberdasher>::new_client(hyper_client, "http://localhost:8080");
        // Try one too small, then too large, then just right
        let work = future::join_all(vec![0, 11, 5].into_iter().map(|inches| {
            service_client
                .make_hat(service::Size { inches }.into())
                .then(move |res| {
                    Ok::<(), ()>(println!(
                        "For size {}: {:?}",
                        inches,
                        res.map(|v| v.output).map_err(|e| e.root_err())
                    ))
                })
        }));
        core.run(work).unwrap();
        shutdown_send.send(()).unwrap();
    }
}

pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(&self, i: service::PTReq<service::Size>) -> service::PTRes<service::Hat> {
        Box::new(future::ok(if i.input.inches < 1 {
            Err(TwirpError::new_meta(
                StatusCode::BadRequest,
                "too_small",
                "Size too small",
                serde_json::to_value(MinMaxSize { min: 1, max: 10 }).ok(),
            )
            .into())
        } else if i.input.inches > 10 {
            Err(TwirpError::new_meta(
                StatusCode::BadRequest,
                "too_large",
                "Size too large",
                serde_json::to_value(MinMaxSize { min: 1, max: 10 }).ok(),
            )
            .into())
        } else {
            Ok(service::Hat {
                size: i.input.inches,
                color: "blue".to_string(),
                name: "fedora".to_string(),
            }
            .into())
        }))
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct MinMaxSize {
    min: i32,
    max: i32,
}
