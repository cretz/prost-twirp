use std::convert::Infallible;
use std::env;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future;

use hyper::http::HeaderValue;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode};
use prost_twirp::{HyperClient, ProstTwirpError, ServiceRequest, ServiceResponse, TwirpError};

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
            let make_service =
                make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });
            let server = Server::bind(&addr).serve(make_service);
            let graceful = server.with_graceful_shutdown(async {
                shutdown_recv.await.ok();
            });
            graceful.await.unwrap();
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
        let prost_client = HyperClient::new(hyper_client, "http://localhost:8080");
        // Run the 5 like the other client
        future::join_all((0..5).map(|_| async {
            let response: ServiceResponse<service::Hat> = prost_client
                .go(
                    "/twirp/twitch.twirp.example.Haberdasher/MakeHat",
                    ServiceRequest::new(service::Size { inches: 12 }),
                )
                .await
                .unwrap();
            let hat: service::Hat = response.output;
            println!("Made {:?}", hat)
        }))
        .await;
        shutdown_send.send(()).unwrap();
    }
}

async fn handle(req: Request<Body>) -> Result<Response<Body>, ProstTwirpError> {
    if req.method() != Method::POST {
        let mut response = TwirpError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Only POST",
        )
        .to_hyper_response();
        response
            .headers_mut()
            .insert("Allow", HeaderValue::from_static("POST"));
        return Ok(response);
    }
    match req.uri().path() {
        "/twirp/twitch.twirp.example.Haberdasher/MakeHat" => {
            let size: service::Size = ServiceRequest::from_hyper_request(req).await?.input;
            ServiceResponse::new(service::Hat {
                size: size.inches,
                color: "blue".to_string(),
                name: "fedora".to_string(),
            })
            .to_hyper_response()
        }
        _ => Ok(
            TwirpError::new(StatusCode::NOT_FOUND, "not_found", "Not found").to_hyper_response(),
        ),
    }
}
