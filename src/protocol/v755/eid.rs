use mcproto_rs::{
    types::VarInt,
    v1_17_0::{
        EntityMetadataFieldData,
        Packet755,
        Packet755Kind,
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

pub fn has_eids(kind: Packet755Kind) -> bool {
    matches!(
        kind,
        Packet755Kind::PlayEntityAnimation
            | Packet755Kind::PlayBlockBreakAnimation
            | Packet755Kind::PlayEntityStatus
            | Packet755Kind::PlayOpenHorseWindow
            | Packet755Kind::PlayEntityPosition
            | Packet755Kind::PlayEntityPositionAndRotation
            | Packet755Kind::PlayEntityRotation
            | Packet755Kind::PlayRemoveEntityEffect
            | Packet755Kind::PlayEntityHeadLook
            | Packet755Kind::PlayCamera
            | Packet755Kind::PlayEntityVelocity
            | Packet755Kind::PlayEntityEquipment
            | Packet755Kind::PlayEntitySoundEffect
            | Packet755Kind::PlayEntityTeleport
            | Packet755Kind::PlayEntityProperties
            | Packet755Kind::PlayEntityEffect
            | Packet755Kind::PlayFacePlayer
            | Packet755Kind::PlayAttachEntity
            | Packet755Kind::PlayEndCombatEvent
            | Packet755Kind::PlayDeathCombatEvent
            | Packet755Kind::PlaySpawnEntity
            | Packet755Kind::PlaySpawnExperienceOrb
            | Packet755Kind::PlaySpawnLivingEntity
            | Packet755Kind::PlaySpawnPainting
            | Packet755Kind::PlaySpawnPlayer
            | Packet755Kind::PlaySetPassengers
            | Packet755Kind::PlayCollectItem
            | Packet755Kind::PlayEntityMetadata
            | Packet755Kind::PlayDestroyEntity
            | Packet755Kind::PlayQueryEntityNbt
            | Packet755Kind::PlayInteractEntity
            | Packet755Kind::PlayEntityAction
            | Packet755Kind::PlayUpdateCommandBlockMinecart
    )
}
pub fn map_eid(
    map: &mut SplinterMapping,
    packet: &mut Packet755,
    sender: &PacketSender,
) -> Option<u64> {
    match sender {
        PacketSender::Server(server, _client) => {
            let mut entity_data: Option<EntityData> = None;
            let (nums, varnums): (Vec<&mut i32>, Vec<&mut VarInt>) = match packet {
                // TODO: is it possible to use something less intensive than a vec here?
                // trivial
                Packet755::PlayEntityAnimation(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayBlockBreakAnimation(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityStatus(body) => (vec![&mut body.entity_id], vec![]),
                Packet755::PlayOpenHorseWindow(body) => (vec![&mut body.entity_id], vec![]),
                Packet755::PlayEntityPosition(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityPositionAndRotation(body) => {
                    (vec![], vec![&mut body.entity_id])
                }
                Packet755::PlayEntityRotation(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayRemoveEntityEffect(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityHeadLook(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayCamera(body) => (vec![], vec![&mut body.camera_id]),
                Packet755::PlayEntityVelocity(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityEquipment(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntitySoundEffect(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityTeleport(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityProperties(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEntityEffect(body) => (vec![], vec![&mut body.entity_id]),
                Packet755::PlayEndCombatEvent(body) => (vec![&mut body.entity_id], vec![]),
                Packet755::PlayDeathCombatEvent(body) => (vec![&mut body.entity_id], vec![]),

                // slightly more complex
                Packet755::PlayFacePlayer(body) => {
                    if let Some(target) = body.entity.as_mut() {
                        (vec![], vec![&mut target.entity_id])
                    } else {
                        (vec![], vec![])
                    }
                }
                Packet755::PlayAttachEntity(body) => (
                    if body.holding_entity_id < 0 {
                        vec![&mut body.attached_entity_id]
                    } else {
                        vec![&mut body.attached_entity_id, &mut body.holding_entity_id]
                    },
                    vec![],
                ),
                Packet755::PlayCollectItem(body) => (
                    vec![],
                    vec![&mut body.collected_entity_id, &mut body.collector_entity_id],
                ),
                Packet755::PlaySetPassengers(body) => {
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
                Packet755::PlaySpawnEntity(body) => {
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
                Packet755::PlaySpawnExperienceOrb(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 24,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                Packet755::PlaySpawnLivingEntity(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: *body.entity_type,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                Packet755::PlaySpawnPainting(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 55,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                Packet755::PlaySpawnPlayer(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 106,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                // complex
                Packet755::PlayEntityMetadata(body) => {
                    // we specially need to handle mapping here for the proxy side eid
                    let proxy_eid = map.map_eid_server_to_proxy(server.id, *body.entity_id);
                    body.entity_id = proxy_eid.into();
                    if let Some(data) = map.entity_data.get(&proxy_eid) {
                        match data.entity_type {
                            27 => {
                                // fireworks
                                if let Some(EntityMetadataFieldData::OptVarInt(ref mut id)) =
                                    body.metadata.get_mut(9)
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
                            97 => {
                                // wither
                                for index in [16, 17, 18] {
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
                                    body.metadata.get_mut(17)
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
                Packet755::PlayDestroyEntity(ref mut body) => {
                    // since we're removing the id from the mapping table here, we have to map them here as well
                    let server_eid = *body.entity_id;
                    body.entity_id = map
                        .map_eid_server_to_proxy(server.id, *body.entity_id)
                        .into();
                    if let Some((proxy_eid, _)) = map.eids.remove_by_right(&(server.id, server_eid))
                    {
                        // debug!("destroying map s->p {} to {}", server_eid, proxy_eid);
                        map.entity_data.remove(&proxy_eid);
                        map.eid_gen.return_id(proxy_eid as u64);
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
                Packet755::PlayQueryEntityNbt(body) => &mut body.entity_id,
                Packet755::PlayInteractEntity(body) => &mut body.entity_id,
                Packet755::PlayEntityAction(body) => &mut body.entity_id,
                Packet755::PlayUpdateCommandBlockMinecart(body) => &mut body.entity_id,
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
