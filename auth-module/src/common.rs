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

use bcrypt::{hash, verify, DEFAULT_COST};
use lazy_static::lazy_static;
use log::{error, info, trace};
use module_utils::pingora::{SessionWrapper, SocketAddr};
use pingora_limits::rate::Rate;
use std::{net::Ipv4Addr, sync::Mutex, time::Duration};

use crate::{AuthConf, AuthRateLimits};

lazy_static! {
    static ref RATE_LIMITER: Mutex<Rate> = Mutex::new(Rate::new(Duration::new(1, 0)));
}

pub(crate) fn is_rate_limited(
    session: &impl SessionWrapper,
    limits: &AuthRateLimits,
    user: &str,
) -> bool {
    if limits.total == 0 && limits.per_user == 0 && limits.per_ip == 0 {
        return false;
    }

    let rate_limiter = match RATE_LIMITER.lock() {
        Ok(rate_limiter) => rate_limiter,
        Err(err) => {
            error!("Failed acquiring rate mutex, rejecting login attempt: {err}");
            return true;
        }
    };

    let mut limited = false;
    if limits.total > 0 && rate_limiter.observe(&(), 1) > limits.total {
        limited = true;
    }
    if limits.per_user > 0 && rate_limiter.observe(&user, 1) > limits.per_user {
        limited = true;
    }
    if limits.per_ip > 0 {
        let ip = session
            .client_addr()
            .and_then(|addr| match addr {
                SocketAddr::Inet(addr) => Some(addr),
                SocketAddr::Unix(_) => None,
            })
            .map(|addr| addr.ip())
            .unwrap_or(Ipv4Addr::new(255, 255, 255, 255).into());
        if rate_limiter.observe(&ip, 1) > limits.per_ip {
            limited = true;
        }
    }
    limited
}

pub(crate) fn validate_login(
    conf: &AuthConf,
    user: &str,
    password: &[u8],
) -> (bool, Option<String>) {
    let result = if let Some(expected) = conf.auth_credentials.get(user) {
        verify(password, expected)
    } else {
        // This user name is unknown. We still go through verification to prevent timing
        // attacks. But we test an empty password against bcrypt-hashed string "test", this is
        // guaranteed to fail.
        verify(
            b"",
            "$2y$12$/GSb/xs3Ss/Jq0zv5qBZWeH3oz8RzEi.PuOhPJ8qiP6yCc2dtDbnK",
        )
    };

    let valid = match result {
        Ok(valid) => valid,
        Err(err) => {
            info!("Rejecting login, bcrypt failure: {err}");
            false
        }
    };

    if !valid {
        info!("Rejecting login, wrong password");
    }

    if !valid && conf.auth_display_hash && !password.is_empty() {
        if let Ok(hash) = hash(password, DEFAULT_COST) {
            trace!("Generated configuration suggestion");
            return (
                valid,
                Some(format!("auth_credentials:\n    \"{user}\": {hash}")),
            );
        }
    }
    (valid, None)
}
