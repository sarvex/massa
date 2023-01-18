// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::net::SocketAddr;

/// gRPC settings.
/// the gRPC settings
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcConfig {
    /// whether to enable gRPC.
    pub enabled: bool,
    /// whether to enable HTTP.
    pub enable_http: bool,
    /// bind for the Massa gRPC API
    pub bind_grpc_api: SocketAddr,
}
