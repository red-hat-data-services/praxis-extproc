// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Shane Utt

//! gRPC health check service for the ExtProc server.
//!
//! Runs on a separate port so Envoy and Kubernetes can probe
//! readiness without going through the ExtProc protocol.

use std::future::Future;

use tracing::info;

/// The `ExternalProcessor` gRPC server type whose serving status is reported.
type ExtProcServer = praxis_proto::envoy::service::ext_proc::v3::external_processor_server::ExternalProcessorServer<
    crate::server::PraxisExtProc,
>;

// -----------------------------------------------------------------------------
// Health Service
// -----------------------------------------------------------------------------

/// Start a gRPC health check server on the given address.
///
/// Registers the `ExternalProcessor` service as `Serving` when `serving` is
/// true, otherwise `NotServing`, and blocks until the provided shutdown
/// future completes.
///
/// # Errors
///
/// Returns a transport error if the server fails to bind or serve.
pub async fn serve(
    addr: std::net::SocketAddr,
    serving: bool,
    shutdown: impl Future<Output = ()>,
) -> Result<(), tonic::transport::Error> {
    let (reporter, svc) = tonic_health::server::health_reporter();

    if serving {
        reporter.set_serving::<ExtProcServer>().await;
    } else {
        reporter.set_not_serving::<ExtProcServer>().await;
    }

    info!(address = %addr, serving, "health server listening");

    tonic::transport::Server::builder()
        .add_service(svc)
        .serve_with_shutdown(addr, shutdown)
        .await
}
