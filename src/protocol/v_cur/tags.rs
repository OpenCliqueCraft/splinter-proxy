use std::{
    collections::HashMap,
    iter::FromIterator,
};

use bimap::BiHashMap;

use crate::{
    current::{
        proto::{
            PlayTagsSpec,
            TagSpec,
            TagType,
            TypedTagList,
        },
        types::{
            CountedArray,
            VarInt,
        },
    },
    protocol::{
        load_json_id_name_pairs,
        TagList,
        Tags,
    },
};

lazy_static! {
    pub static ref BLOCK_MAP: BiHashMap<i32, String> =
        BiHashMap::<i32, String>::from_iter(load_json_id_name_pairs(include_str!(
            "../../../minecraft-data/data/pc/1.17/blocks.json"
        )));
    pub static ref ITEM_MAP: BiHashMap<i32, String> =
        BiHashMap::<i32, String>::from_iter(load_json_id_name_pairs(include_str!(
            "../../../minecraft-data/data/pc/1.17/items.json"
        )));
    pub static ref ENTITY_MAP: BiHashMap<i32, String> =
        BiHashMap::<i32, String>::from_iter(load_json_id_name_pairs(include_str!(
            "../../../minecraft-data/data/pc/1.17/entities.json"
        )));
    pub static ref FLUID_MAP: BiHashMap<i32, String> = BiHashMap::<i32, String>::from_iter(
        load_json_id_name_pairs(include_str!("../../../fluids.json")),
    );
    pub static ref GAME_EVENT_MAP: BiHashMap<i32, String> = BiHashMap::<i32, String>::from_iter(
        load_json_id_name_pairs(include_str!("../../../game_events.json")),
    );
}

pub fn proto_tags_to_tags(
    proto_tags: &CountedArray<TagSpec, VarInt>,
    map: &BiHashMap<i32, String>,
) -> TagList {
    let mut list = HashMap::new();
    for tag in proto_tags.iter() {
        list.insert(
            tag.name.clone(),
            tag.entries
                .iter()
                .map(|val| map.get_by_left(&**val).unwrap().clone())
                .collect::<Vec<String>>(),
        );
    }
    TagList(list)
}

pub fn tags_to_proto_tags(
    tags: &TagList,
    map: &BiHashMap<i32, String>,
) -> CountedArray<TagSpec, VarInt> {
    let mut list = vec![];
    for (name, ids) in tags.0.iter() {
        list.push(TagSpec {
            name: name.clone(),
            entries: ids
                .iter()
                .map(|id| VarInt::from(*map.get_by_right(id).unwrap()))
                .collect::<Vec<VarInt>>()
                .into(),
        });
    }
    list.into()
}

impl From<&PlayTagsSpec> for Tags {
    fn from(proto_tags: &PlayTagsSpec) -> Tags {
        let mut tags_map = HashMap::new();
        for typed_tags in proto_tags.tags.iter() {
            let (type_name, map) = match &typed_tags.tag_type {
                TagType::Block => ("minecraft:block", &*BLOCK_MAP),
                TagType::Item => ("minecraft:item", &*ITEM_MAP),
                TagType::Fluid => ("minecraft:fluid", &*FLUID_MAP),
                TagType::EntityType => ("minecraft:entity_type", &*ENTITY_MAP),
                TagType::GameEvent => ("minecraft:game_event", &*GAME_EVENT_MAP),
            };
            tags_map.insert(type_name.into(), proto_tags_to_tags(&typed_tags.tags, map));
        }
        Tags {
            tags: tags_map,
        }
    }
}

impl From<&Tags> for PlayTagsSpec {
    fn from(tags: &Tags) -> PlayTagsSpec {
        let mut typed_tags = vec![];
        for (name, tag_list) in tags.tags.iter() {
            let (tag_type, map) = match name.as_str() {
                "minecraft:block" => (TagType::Block, &*BLOCK_MAP),
                "minecraft:item" => (TagType::Item, &*ITEM_MAP),
                "minecraft:fluid" => (TagType::Fluid, &*FLUID_MAP),
                "minecraft:entity_type" => (TagType::EntityType, &*ENTITY_MAP),
                "minecraft:game_event" => (TagType::GameEvent, &*GAME_EVENT_MAP),
                _ => continue,
            };
            typed_tags.push(TypedTagList {
                tag_type,
                tags: tags_to_proto_tags(tag_list, map),
            })
        }
        PlayTagsSpec {
            tags: typed_tags.into(),
        }
    }
}
