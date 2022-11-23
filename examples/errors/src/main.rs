use std::convert::Infallible;
use std::env;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future;
use hyper::server::Server;
use hyper::service::make_service_fn;
use hyper::Client;
use hyper::StatusCode;
use serde_derive::{Deserialize, Serialize};

use prost_twirp::TwirpError;

mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}

#[tokio::main]
async fn main() {
    let run_server = env::args().any(|s| s == "--server");
    let run_client = !run_server || env::args().any(|s| s == "--client");
    let (shutdown_send, shutdown_recv) = oneshot::channel::<()>();

    if run_server {
        let thread_res = tokio::spawn(async {
            println!("Starting server");
            let addr = "0.0.0.0:8080".parse().unwrap();
            let server = Server::bind(&addr)
                .serve(make_service_fn(|_conn| async {
                    Ok::<_, Infallible>(<dyn service::Haberdasher>::new_server(HaberdasherService))
                }))
                .with_graceful_shutdown(async {
                    shutdown_recv.await.ok();
                });
            server.await.unwrap();
            println!("Server stopped");
        });
        // Wait a sec or forever depending on whether there's client code to run
        if run_client {
            tokio::time::sleep(Duration::from_millis(1000)).await;
        } else {
            thread_res.await.unwrap();
        }
    }

    if run_client {
        let hyper_client = Client::new();
        let service_client = std::sync::Arc::new(<dyn service::Haberdasher>::new_client(
            hyper_client,
            "http://localhost:8080",
        ));
        // Try one too small, then too large, then just right
        future::join_all(vec![0, 11, 5].into_iter().map(|inches| {
            let service_client = Arc::clone(&service_client);
            async move {
                let res = service_client
                    .make_hat(service::Size { inches }.into())
                    .await;
                println!(
                    "For size {}: {:?}",
                    inches,
                    res.map(|v| v.output).map_err(|e| e.root_err())
                );
            }
        }))
        .await;
        shutdown_send.send(()).unwrap();
    }
}

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

#[derive(Serialize, Deserialize, Debug)]
struct MinMaxSize {
    min: i32,
    max: i32,
}
