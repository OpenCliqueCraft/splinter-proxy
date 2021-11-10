use std::{sync::Arc, time::Duration};

use smallvec::SmallVec;
use smol::Timer;

use crate::{proxy::SplinterProxy, systems::SplinterSystem};

pub enum Zone {
    Rectangle { x1: i32, z1: i32, x2: i32, z2: i32 },
    InvertedRectangle { x1: i32, z1: i32, x2: i32, z2: i32 },
}

pub fn world_to_chunk_position((x, z): (f64, f64)) -> (i32, i32) {
    return ((x as i32) >> 4, (z as i32) >> 4);
}

impl Zone {
    pub fn point_in_zone(&self, x: i32, z: i32) -> bool {
        match self {
            Self::Rectangle { x1, z1, x2, z2 } => x >= *x1 && x < *x2 && z >= *z1 && z < *z2,
            Self::InvertedRectangle { x1, z1, x2, z2 } => {
                !(x >= *x1 && x < *x2 && z >= *z1 && z < *z2)
            }
        }
    }
}

pub struct Zoner {
    pub zones: Vec<(u64, Zone)>,
}

impl Zoner {
    pub fn zones_in_point(&self, (x, z): (i32, i32)) -> SmallVec<[u64; 2]> {
        let mut ids = SmallVec::new();
        for (server_id, zone) in self.zones.iter() {
            if zone.point_in_zone(x, z) {
                ids.push(*server_id);
            }
        }
        return ids;
    }
}

inventory::submit! {
    SplinterSystem {
        name: "Zoner",
        init: Box::new(|proxy| {
            Box::pin(async move {
                smol::spawn(async move {
                    if let Err(e) = zoner_loop(proxy).await {
                        error!("Zoner encountered an error: {:?}", e);
                    }
                }).detach();
                Ok(())
            })
        }),
    }
}

pub async fn zoner_loop(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    loop {
        Timer::after(Duration::from_secs(1)).await;
        for (_, cl) in proxy.players.read().await.iter() {
            let pl_pos = &**cl.position.load();
            if let Err(e) = cl
                .update_touching_servers(
                    proxy
                        .zoner
                        .zones_in_point(world_to_chunk_position((pl_pos.x, pl_pos.z))),
                )
                .await
            {
                error!(
                    "Error updating touching servers for player {}: {:?}",
                    &cl.name, e
                );
            }
        }
    }
}
