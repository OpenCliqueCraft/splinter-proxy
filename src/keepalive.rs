use std::{
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use smol::Timer;

use crate::{
    init::SplinterSystem,
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
};
inventory::submit! {
    SplinterSystem {
        name: "Keep Alive",
        init: Box::new(|proxy| {
            Box::pin(keep_alive_loop(proxy))
        })
    }
}

async fn keep_alive_loop(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    smol::spawn(async move {
        loop {
            Timer::after(Duration::from_secs(15)).await;
            let players = proxy
                .players
                .read()
                .await
                .iter()
                .map(|(_, client)| Arc::clone(client))
                .collect::<Vec<_>>();
            let keep_alive_millis = unix_time_millis();
            for client in players.iter() {
                if keep_alive_millis - *client.last_keep_alive.lock().await > 30 * 1000 {
                    // client connection time out
                    if let Err(e) = proxy
                        .kick_client(&client.name, ClientKickReason::TimedOut)
                        .await
                    {
                        error!(
                            "Error while kicking timed out client \"{}\": {}",
                            &client.name, e
                        );
                    }
                }
            }
            let send_futs = players
                .iter()
                .map(|client| {
                    let fut = client.send_keep_alive(keep_alive_millis);
                    (client, fut)
                })
                .collect::<Vec<_>>()
                .into_iter();

            for (client, fut) in send_futs {
                if let Err(e) = fut.await {
                    error!(
                        "Failed to send keep alive packet to client \"{}\": {}",
                        &client.name, e
                    );
                }
            }
        }
    })
    .detach();
    Ok(())
}

/// Gets the current unix time in milliseconds
pub fn unix_time_millis() -> u128 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(e) => {
            warn!("System time before unix epoch?: {}", e);
            0
        }
    }
}
