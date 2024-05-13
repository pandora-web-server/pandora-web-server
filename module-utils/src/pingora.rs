// Copyright 2024 Wladimir Palant
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Exposes some types from `pingora-core` and `pingora-proxy` crates, so that typical modules no
//! longer need them as direct dependencies.

pub use pingora_core::protocols::http::HttpTask;
pub use pingora_core::upstreams::peer::HttpPeer;
pub use pingora_core::{Error, ErrorType};
pub use pingora_http::ResponseHeader;
pub use pingora_proxy::Session;
