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

//! Unix signal processing

use log::{error, warn};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc::Sender;

use crate::writer::WriterMessage;

fn listen_to_signal(kind: SignalKind, sender: Sender<WriterMessage>) {
    tokio::spawn(async move {
        let mut sig = match signal(kind) {
            Ok(sig) => sig,
            Err(err) => {
                warn!(
                    "Failed registering for signal {}: {err}",
                    kind.as_raw_value()
                );
                return;
            }
        };

        loop {
            sig.recv().await;
            if let Err(err) = sender.send(WriterMessage::Reopen).await {
                error!("Failed reopening log files, thread crashed? {err}");
            }
        }
    });
}

pub(crate) fn listen(sender: &Sender<WriterMessage>) {
    listen_to_signal(SignalKind::hangup(), sender.clone());
    listen_to_signal(SignalKind::user_defined1(), sender.clone());
}
