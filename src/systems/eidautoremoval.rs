use std::{
    collections::HashSet,
    sync::Arc,
    time::Duration,
};

use smol::Timer;

use crate::{
    proxy::SplinterProxy,
    systems::SplinterSystem,
};

inventory::submit! {
    SplinterSystem {
        name: "Entity ID Auto Removal",
        init: Box::new(|proxy| {
            Box::pin(eid_auto_removal_loop(proxy))
        }),
    }
}

async fn eid_auto_removal_loop(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    smol::spawn(async move {
        loop {
            Timer::after(Duration::from_secs(15)).await;
            let mut total_used_eids = HashSet::<i32>::new();
            for (_, cl) in proxy.players.read().await.iter() {
                let cl_known_eids = cl.known_eids.lock().await;
                for eid in cl_known_eids.iter() {
                    total_used_eids.insert(*eid);
                }
            }
            let map = &mut *proxy.mapping.lock().await;
            let eids_for_removal = map
                .eids
                .iter()
                .map(|(e, _)| *e)
                .filter(|e| !total_used_eids.contains(e))
                .collect::<Vec<i32>>();
            // if there is no reference among any client to an eid, then we dont need that
            // mapping
            for proxy_eid in eids_for_removal {
                if let Some((_, (server_id, server_eid))) = map.eids.remove_by_left(&proxy_eid) {
                    debug!(
                        "destroying map s->p ({}, {}) to {}",
                        server_id, server_eid, proxy_eid
                    );
                    map.entity_data.remove(&proxy_eid);
                    map.eid_gen.return_id(proxy_eid as u64);
                }
            }
        }
    })
    .detach();
    Ok(())
}
