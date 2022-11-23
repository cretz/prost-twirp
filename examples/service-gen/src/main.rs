use std::convert::Infallible;
use std::env;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future;
use hyper::server::Server;
use hyper::service::make_service_fn;
use hyper::Client;

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
        let service_client =
            <dyn service::Haberdasher>::new_client(hyper_client, "http://localhost:8080");
        future::join_all((0..5).map(|_| async {
            let res = service_client
                .make_hat(service::Size { inches: 12 }.into())
                .await
                .unwrap();
            println!("Made {:?}", res.output);
        }))
        .await;
        shutdown_send.send(()).unwrap();
    }
}

pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(
        &self,
        req: service::ServiceRequest<service::Size>,
    ) -> service::PTRes<service::Hat> {
        Box::pin(future::ok(
            service::Hat {
                size: req.input.inches,
                color: "blue".to_string(),
                name: "fedora".to_string(),
            }
            .into(),
        ))
    }
}
