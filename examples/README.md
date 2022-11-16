# Examples

* [service-gen](service-gen) - Example showing how to generate service code that uses `prost-twirp` as a runtime
  dependency.
* [service-gen-no-runtime](service-gen-no-runtime) - Example showing how to generate service code and embed the runtime
  code to not have `prost-twirp` as a dependency.
* [errors](errors) - Example showing some error handling.
* [no-service-gen](no-service-gen) - Example showing how to use `prost-twirp` as a runtime dependency manually without
  any code generation for the service.

The examples use the [service.proto](service.proto) file that is used by the
[Twirp examples](https://github.com/twitchtv/twirp/tree/master/example). To run any command, just navigate to the dir
and type:

    cargo run

This will run the example client that will send requests to a server that is already running. To run both the client and
the server in the example, use:

    cargo run -- --server --client

This starts the server, runs the client, and then closes the server. To run just the server to be connected by an
external client, use:

    cargo run -- --server
