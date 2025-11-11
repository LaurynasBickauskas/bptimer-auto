use crate::live::bptimer_stream::{
    BPTIMER_BASE_URL, CREATE_HP_REPORT_ENDPOINT, CROWD_SOURCE_API_KEY,
};
use crate::live::opcodes_models::{
    attr_type, Encounter, Entity, MONSTER_NAMES, MONSTER_NAMES_CROWDSOURCE,
    MONSTER_UID_CROWDSOURCE_MAP,
};
use crate::packets::utils::BinaryReader;
use blueprotobuf_lib::blueprotobuf;
use log::{error, info, warn};
use std::default::Default;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn on_server_change(encounter: &mut Encounter) {
    info!("on server change");
    encounter.clone_from(&Encounter::default());
}

pub fn process_sync_near_entities(
    encounter: &mut Encounter,
    sync_near_entities: blueprotobuf::SyncNearEntities,
) -> Option<()> {
    for pkt_entity in sync_near_entities.appear {
        let target_uuid = pkt_entity.uuid?;
        let target_uid = target_uuid >> 16;
        let target_entity_type = blueprotobuf::EEntityType::from(target_uuid);

        let target_entity = encounter
            .entity_uid_to_entity
            .entry(target_uid)
            .or_default();
        target_entity.entity_type = target_entity_type;

        match target_entity_type {
            blueprotobuf::EEntityType::EntChar => process_player_attrs(target_entity, target_uid, pkt_entity.attrs?.attrs),
            blueprotobuf::EEntityType::EntMonster => process_monster_attrs(target_entity, pkt_entity.attrs?.attrs, encounter.local_player.as_ref()),
            _ => {}
        }
    }
    Some(())
}

pub fn process_sync_container_data(
    encounter: &mut Encounter,
    sync_container_data: blueprotobuf::SyncContainerData,
) -> Option<()> {
    let v_data = sync_container_data.v_data?;
    let player_uid = v_data.char_id?;

    let target_entity = encounter
        .entity_uid_to_entity
        .entry(player_uid)
        .or_default();
    let char_base = v_data.char_base?;
    target_entity.name = Some(char_base.name?);
    target_entity.entity_type = blueprotobuf::EEntityType::EntChar;
    Some(())
}

pub fn process_sync_to_me_delta_info(
    encounter: &mut Encounter,
    sync_to_me_delta_info: blueprotobuf::SyncToMeDeltaInfo,
) -> Option<()> {
    let delta_info = sync_to_me_delta_info.delta_info?;
    encounter.local_player_uid = Some(delta_info.uuid? >> 16);
    process_aoi_sync_delta(encounter, delta_info.base_delta?);
    Some(())
}

pub fn process_aoi_sync_delta(
    encounter: &mut Encounter,
    aoi_sync_delta: blueprotobuf::AoiSyncDelta,
) -> Option<()> {
    let target_uuid = aoi_sync_delta.uuid?;
    let target_uid = target_uuid >> 16;

    let target_entity_type = blueprotobuf::EEntityType::from(target_uuid);
    {
        let target_entity = encounter
            .entity_uid_to_entity
            .entry(target_uid)
            .or_insert_with(|| Entity {
                entity_type: target_entity_type,
                ..Default::default()
            });

        if let Some(attrs_collection) = aoi_sync_delta.attrs {
            match target_entity_type {
                blueprotobuf::EEntityType::EntChar => process_player_attrs(target_entity, target_uid, attrs_collection.attrs),
                blueprotobuf::EEntityType::EntMonster => process_monster_attrs(target_entity, attrs_collection.attrs, encounter.local_player.as_ref()),
                _ => {}
            }
        }
    }

    let Some(skill_effect) = aoi_sync_delta.skill_effects else {
        return Some(()); 
    };

    for _ in skill_effect.damages {
        let target_entity = encounter.entity_uid_to_entity.get(&target_uid);
        let monster_id = target_entity.and_then(|e| e.monster_id);
        let is_crowdsource = monster_id
            .is_some_and(|id| MONSTER_NAMES_CROWDSOURCE.contains_key(&id));
        let crowdsource_name = if is_crowdsource {
        monster_id
                .and_then(|id| MONSTER_NAMES.get(&id))
                .or_else(|| target_entity.and_then(|e| e.name.as_ref()))
                .cloned()
        } else {
            None
        };
        let crowdsource_remote_id = monster_id
            .and_then(|id| MONSTER_UID_CROWDSOURCE_MAP.get(&id).cloned());

        if is_crowdsource {

            if crowdsource_remote_id.is_none() {
                warn!(
                    "live::opcodes_process::handle_damage_packet - crowdsourced monster missing remote id for monster_id={monster_id:?}, monster_name={crowdsource_name:?}"
                );
            }
            encounter.crowdsource_monster_name = crowdsource_name.clone();
            encounter.crowdsource_monster_id = monster_id;
            encounter.crowdsource_monster_remote_id = crowdsource_remote_id.clone();
        }
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis();
    if encounter.time_fight_start_ms == Default::default() {
        encounter.time_fight_start_ms = timestamp_ms;
    }
    encounter.time_last_combat_packet_ms = timestamp_ms;
    Some(())
}


fn process_player_attrs(player_entity: &mut Entity, player_uid: i64, attrs: Vec<blueprotobuf::Attr>) {
    for attr in attrs {
        let Some(mut raw_bytes) = attr.raw_data else {
            continue;
        };
        let Some(attr_id) = attr.id else { continue; };

        match attr_id {
            attr_type::ATTR_NAME => {
                raw_bytes.remove(0);
                let player_name_result = BinaryReader::from(raw_bytes).read_string();
                if let Ok(player_name) = player_name_result {
                    player_entity.name = Some(player_name.clone());
                    info!("Found player {player_name} with UID {player_uid}");
                } else {
                    warn!("Failed to read player name for UID {player_uid}");
                }
            }
            _ => (),
        }
    }
}

fn process_monster_attrs(
    monster_entity: &mut Entity,
    attrs: Vec<blueprotobuf::Attr>,
    local_player: Option<&blueprotobuf::SyncContainerData>,
) {
    for attr in attrs {
        let Some(raw_bytes) = attr.raw_data else { continue; };
        let Some(attr_id) = attr.id else { continue; };

        #[allow(clippy::cast_possible_truncation)]
        match attr_id {
            attr_type::ATTR_ID => monster_entity.monster_id = Some(prost::encoding::decode_varint(&mut raw_bytes.as_slice()).unwrap() as i32),
            attr_type::ATTR_HP => {
                let curr_hp = prost::encoding::decode_varint(&mut raw_bytes.as_slice()).unwrap() as i32;
                let prev_hp_opt = monster_entity.curr_hp;
                monster_entity.curr_hp = Some(curr_hp);

                let endpoint = format!("{BPTIMER_BASE_URL}{CREATE_HP_REPORT_ENDPOINT}");
                let (Some(monster_id), Some(local_player)) = (monster_entity.monster_id, &local_player) else {
                    continue;
                };
                let Some(max_hp) = monster_entity.max_hp else {
                    continue;
                };
                if MONSTER_NAMES_CROWDSOURCE.contains_key(&monster_id) { // only record if it's a world boss, magical creature, etc.
                    let monster_name = MONSTER_NAMES.get(&monster_id).map_or("Unknown Monster Name", |s| s.as_str());
                    let new_hp_pct = (curr_hp * 100 / max_hp).clamp(0, 100);
                    let old_hp_pct_opt =
                        prev_hp_opt.map(|prev_hp| (prev_hp * 100 / max_hp).clamp(0, 100));
                    let Some((Some(line), Some(pos_x), Some(pos_y))) = local_player.v_data.as_ref()
                                                                               .and_then(|v| v.scene_data.as_ref())
                                                                               .map(|s| (
                                                                                   s.line_id,
                                                                                   s.pos.as_ref().and_then(|p| p.x),
                                                                                   s.pos.as_ref().and_then(|p| p.y),
                                                                               ))
                    else {
                        continue;
                    };

                    let should_report = match old_hp_pct_opt {
                        None => true,
                        Some(old_hp_pct) => old_hp_pct != new_hp_pct && new_hp_pct % 5 == 0,
                    };

                    if should_report {
                        info!("Found crowdsourced monster with Name {monster_name} - ID {monster_id} - HP% {new_hp_pct}% on line {line} and pos ({pos_x},{pos_y})");
                        let body = serde_json::json!({
                            "monster_id": monster_id,
                            "hp_pct": new_hp_pct,
                            "line": line,
                            "pos_x": pos_x,
                            "pos_y": pos_y,
                        });
                        tokio::spawn(async move {
                            let endpoint = endpoint.clone();
                            let client = reqwest::Client::new();
                            let res = client
                                .post(endpoint)
                                .header("X-API-Key", CROWD_SOURCE_API_KEY)
                                .json(&body)
                                .send().await;
                            match res {
                                Ok(resp) => {
                                    if resp.status() != reqwest::StatusCode::OK {
                                        error!("POST monster info failed: status {}", resp.status());
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to POST monster info: {e}");
                                }
                            }
                        });
                    }
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            attr_type::ATTR_MAX_HP => monster_entity.max_hp = Some(prost::encoding::decode_varint(&mut raw_bytes.as_slice()).unwrap() as i32),
            _ => (),
        }
    }
}
