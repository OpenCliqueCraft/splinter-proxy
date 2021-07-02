use mcproto_rs::{
    types::VarInt,
    v1_16_3::{
        EntityMetadataFieldData,
        Packet753,
        Packet753Kind,
        RawPacket753,
    },
};

use super::RelayPass;
use crate::{
    mapping::{
        EntityData,
        SplinterMapping,
    },
    protocol::{
        PacketDestination,
        PacketSender,
    },
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, sender, lazy_packet, map, destination| {
        if has_eids(lazy_packet.kind()) {
            if let Ok(ref mut packet) = lazy_packet.packet() {
                if let Some(server_id) = map_eid(map, packet, sender) {
                    *destination = PacketDestination::Server(server_id);
                }
            }
        }
    }))
}

pub fn has_eids(kind: Packet753Kind) -> bool {
    matches!(
        kind,
        Packet753Kind::PlayEntityAnimation
            | Packet753Kind::PlayBlockBreakAnimation
            | Packet753Kind::PlayEntityStatus
            | Packet753Kind::PlayOpenHorseWindow
            | Packet753Kind::PlayEntityPosition
            | Packet753Kind::PlayEntityPositionAndRotation
            | Packet753Kind::PlayEntityRotation
            | Packet753Kind::PlayEntityMovement
            | Packet753Kind::PlayRemoveEntityEffect
            | Packet753Kind::PlayEntityHeadLook
            | Packet753Kind::PlayCamera
            | Packet753Kind::PlayEntityVelocity
            | Packet753Kind::PlayEntityEquipment
            | Packet753Kind::PlayEntitySoundEffect
            | Packet753Kind::PlayEntityTeleport
            | Packet753Kind::PlayEntityProperties
            | Packet753Kind::PlayEntityEffect
            | Packet753Kind::PlayFacePlayer
            | Packet753Kind::PlayAttachEntity
            | Packet753Kind::PlaySpawnEntity
            | Packet753Kind::PlaySpawnExperienceOrb
            | Packet753Kind::PlaySpawnLivingEntity
            | Packet753Kind::PlaySpawnPainting
            | Packet753Kind::PlaySpawnPlayer
            | Packet753Kind::PlaySetPassengers
            | Packet753Kind::PlayCollectItem
            | Packet753Kind::PlayEntityMetadata
            | Packet753Kind::PlayDestroyEntities
            | Packet753Kind::PlayQueryEntityNbt
            | Packet753Kind::PlayInteractEntity
            | Packet753Kind::PlayEntityAction
            | Packet753Kind::PlayUpdateCommandBlockMinecart
    )
}
pub fn map_eid(
    map: &mut SplinterMapping,
    packet: &mut Packet753,
    sender: &PacketSender,
) -> Option<u64> {
    match sender {
        PacketSender::Server(server) => {
            let mut entity_data: Option<EntityData> = None;
            let (nums, varnums): (Vec<&mut i32>, Vec<&mut VarInt>) = match packet {
                // TODO: is it possible to use something less intensive than a vec here?
                // trivial
                Packet753::PlayEntityAnimation(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayBlockBreakAnimation(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityStatus(body) => (vec![&mut body.entity_id], vec![]),
                Packet753::PlayOpenHorseWindow(body) => (vec![&mut body.entity_id], vec![]),
                Packet753::PlayEntityPosition(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityPositionAndRotation(body) => {
                    (vec![], vec![&mut body.entity_id])
                }
                Packet753::PlayEntityRotation(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityMovement(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayRemoveEntityEffect(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityHeadLook(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayCamera(body) => (vec![], vec![&mut body.camera_id]),
                Packet753::PlayEntityVelocity(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityEquipment(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntitySoundEffect(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityTeleport(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityProperties(body) => (vec![], vec![&mut body.entity_id]),
                Packet753::PlayEntityEffect(body) => (vec![], vec![&mut body.entity_id]),

                // slightly more complex
                Packet753::PlayFacePlayer(body) => {
                    if let Some(target) = body.entity.as_mut() {
                        (vec![], vec![&mut target.entity_id])
                    } else {
                        (vec![], vec![])
                    }
                }
                Packet753::PlayAttachEntity(body) => (
                    if body.holding_entity_id < 0 {
                        vec![&mut body.attached_entity_id]
                    } else {
                        vec![&mut body.attached_entity_id, &mut body.holding_entity_id]
                    },
                    vec![],
                ),
                Packet753::PlayCollectItem(body) => (
                    vec![],
                    vec![&mut body.collected_entity_id, &mut body.collector_entity_id],
                ),
                Packet753::PlaySetPassengers(body) => {
                    // TODO: spelling error in mcproto
                    (
                        vec![],
                        body.passenger_entitiy_ids.iter_mut().fold(
                            vec![&mut body.entity_id],
                            |mut acc, item| {
                                acc.push(item);
                                acc
                            },
                        ),
                    )
                }

                // entity spawning
                Packet753::PlaySpawnEntity(body) => {
                    let entity_type = *body.entity_type;
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type,
                    });
                    // debug!("entity spawn type: {}", entity_type);
                    (
                        match entity_type {
                            107 => {
                                // bobber
                                vec![&mut body.data]
                            }
                            2 | 79 | 39 | 76 | 15 | 99 => {
                                // arrow, spectral arrow, fireball, small fireball, dragon fireball, wither skull
                                if body.data > 0 {
                                    // body.data is option varint. we need to specially handle this
                                    body.data =
                                        map.map_eid_server_to_proxy(server.id, body.data - 1) + 1;
                                }
                                vec![]
                            }
                            _ => {
                                // debug!("got type entity spawn type without eid {}", entity_type);
                                vec![]
                            }
                        },
                        vec![&mut body.entity_id],
                    )
                }
                Packet753::PlaySpawnExperienceOrb(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 24,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                Packet753::PlaySpawnLivingEntity(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: *body.entity_type,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                Packet753::PlaySpawnPainting(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 55,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                Packet753::PlaySpawnPlayer(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 106,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                // complex
                Packet753::PlayEntityMetadata(body) => {
                    // we specially need to handle mapping here for the proxy side eid
                    let proxy_eid = map.map_eid_server_to_proxy(server.id, *body.entity_id);
                    body.entity_id = proxy_eid.into();
                    if let Some(data) = map.entity_data.get(&proxy_eid) {
                        match data.entity_type {
                            27 => {
                                // fireworks
                                if let Some(EntityMetadataFieldData::OptVarInt(ref mut id)) =
                                    body.metadata.get_mut(8)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        *id = (map
                                            .map_eid_server_to_proxy(server.id, found_id - 1)
                                            + 1)
                                        .into();
                                    }
                                }
                            }
                            107 => {
                                // fishing hook
                                if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                    body.metadata.get_mut(7)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        *id = (map
                                            .map_eid_server_to_proxy(server.id, found_id - 1)
                                            + 1)
                                        .into();
                                    }
                                }
                            }
                            97 => {
                                // wither
                                for index in [15, 16, 17] {
                                    if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                        body.metadata.get_mut(index)
                                    {
                                        let found_id: i32 = **id;
                                        if found_id > 0 {
                                            *id = (map
                                                .map_eid_server_to_proxy(server.id, found_id - 1)
                                                + 1) // docs dont say + 1, but Im assuming that is the case here
                                            .into();
                                        }
                                    }
                                }
                            }
                            31 | 17 => {
                                // guardian or elder guardian
                                if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                    body.metadata.get_mut(16)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        *id = (map
                                            .map_eid_server_to_proxy(server.id, found_id - 1)
                                            + 1)
                                        .into();
                                        // docs dont say +1, same as above
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    (vec![], vec![])
                }
                Packet753::PlayDestroyEntities(body) => {
                    for eid in body.entity_ids.iter_mut() {
                        // since we're removing the ids from the mapping table here, we have to map them here as well
                        let server_eid = **eid;
                        *eid = map.map_eid_server_to_proxy(server.id, **eid).into();
                        if let Some((proxy_eid, _)) =
                            map.eids.remove_by_right(&(server.id, server_eid))
                        {
                            // debug!("destroying map s->p {} to {}", server_eid, proxy_eid);
                            map.entity_data.remove(&proxy_eid);
                            map.eid_gen.return_id(proxy_eid as u64);
                        }
                    }
                    (vec![], vec![])
                }
                _ => unreachable!(),
            };
            for id in nums {
                *id = map.map_eid_server_to_proxy(server.id, *id);
            }
            for id in varnums {
                *id = map.map_eid_server_to_proxy(server.id, **id).into();
            }
            if let Some(mut data) = entity_data {
                let proxy_eid = map.map_eid_server_to_proxy(server.id, data.id); // this should get the same id we just generated
                data.id = proxy_eid;
                map.entity_data.insert(proxy_eid, data);
            }
        }
        PacketSender::Proxy(_) => {
            let eid = match packet {
                Packet753::PlayQueryEntityNbt(body) => &mut body.entity_id,
                Packet753::PlayInteractEntity(body) => &mut body.entity_id,
                Packet753::PlayEntityAction(body) => &mut body.entity_id,
                Packet753::PlayUpdateCommandBlockMinecart(body) => &mut body.entity_id,
                _ => unreachable!(),
            };
            if let Ok((server_id, server_eid)) = map.map_eid_proxy_to_server(**eid) {
                *eid = server_eid.into();
                return Some(server_id);
            }
        }
    };
    None
}
