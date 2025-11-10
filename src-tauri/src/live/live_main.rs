use crate::live::crowdsource_persistence::{
    apply_snapshot_to_encounter, load_snapshot, save_snapshot, snapshot_from_encounter,
};
use crate::live::opcodes_models::EncounterMutex;
use crate::live::opcodes_process::{
    on_server_change, process_aoi_sync_delta, process_sync_container_data,
    process_sync_near_entities, process_sync_to_me_delta_info,
};
use crate::packets;
use blueprotobuf_lib::blueprotobuf;
use bytes::Bytes;
use log::{info, warn};
use prost::Message;
use tauri::{AppHandle, Manager};

pub async fn start(app_handle: AppHandle) {
    let mut rx = packets::packet_capture::start_capture();

    while let Some((op, data)) = rx.recv().await {
        {
            let state = app_handle.state::<EncounterMutex>();
            let encounter = state.lock().unwrap();
            if encounter.is_encounter_paused {
                info!("packet dropped due to encounter paused");
                continue;
            }
        }
        match op {
            packets::opcodes::Pkt::ServerChangeInfo => {
                let encounter_state = app_handle.state::<EncounterMutex>();
                let mut encounter_state = encounter_state.lock().unwrap();
                on_server_change(&mut encounter_state);

                if let Some(snapshot) = load_snapshot(&app_handle) {
                    apply_snapshot_to_encounter(&snapshot, &mut encounter_state);
                }
            }
            packets::opcodes::Pkt::SyncNearEntities => {
                let sync_near_entities =
                    match blueprotobuf::SyncNearEntities::decode(Bytes::from(data)) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Error decoding SyncNearEntities.. ignoring: {e}");
                            continue;
                        }
                    };
                let encounter_state = app_handle.state::<EncounterMutex>();
                let mut encounter_state = encounter_state.lock().unwrap();
                let previous_remote_id = encounter_state.crowdsource_monster_remote_id.clone();
                let mut snapshot_to_save = None;
                if process_sync_near_entities(&mut encounter_state, sync_near_entities).is_none() {
                    warn!("Error processing SyncNearEntities.. ignoring.");
                }
                if encounter_state.crowdsource_monster_remote_id != previous_remote_id {
                    snapshot_to_save = snapshot_from_encounter(&encounter_state);
                }
                drop(encounter_state);
                if let Some(snapshot) = snapshot_to_save {
                    if let Err(err) = save_snapshot(&app_handle, &snapshot) {
                        warn!("live_main::start - failed to persist crowdsourced monster snapshot: {err}");
                    }
                }
            }
            packets::opcodes::Pkt::SyncContainerData => {
                let sync_container_data =
                    match blueprotobuf::SyncContainerData::decode(Bytes::from(data)) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Error decoding SyncContainerData.. ignoring: {e}");
                            continue;
                        }
                    };
                let encounter_state = app_handle.state::<EncounterMutex>();
                let mut encounter_state = encounter_state.lock().unwrap();
                let previous_remote_id = encounter_state.crowdsource_monster_remote_id.clone();
                encounter_state.local_player = Some(sync_container_data.clone());
                if process_sync_container_data(&mut encounter_state, sync_container_data).is_none() {
                    warn!("Error processing SyncContainerData.. ignoring.");
                }
                let mut snapshot_to_save = None;
                if encounter_state.crowdsource_monster_remote_id != previous_remote_id {
                    snapshot_to_save = snapshot_from_encounter(&encounter_state);
                }
                drop(encounter_state);
                if let Some(snapshot) = snapshot_to_save {
                    if let Err(err) = save_snapshot(&app_handle, &snapshot) {
                        warn!("live_main::start - failed to persist crowdsourced monster snapshot: {err}");
                    }
                }
            }
            packets::opcodes::Pkt::SyncServerTime => {
                let _sync_server_time =
                    match blueprotobuf::SyncServerTime::decode(Bytes::from(data)) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Error decoding SyncServerTime.. ignoring: {e}");
                            continue;
                        }
                    };
            }
            packets::opcodes::Pkt::SyncToMeDeltaInfo => {
                let sync_to_me_delta_info =
                    match blueprotobuf::SyncToMeDeltaInfo::decode(Bytes::from(data)) {
                        Ok(sync_to_me_delta_info) => sync_to_me_delta_info,
                        Err(e) => {
                            warn!("Error decoding SyncToMeDeltaInfo.. ignoring: {e}");
                            continue;
                        }
                    };
                let encounter_state = app_handle.state::<EncounterMutex>();
                let mut encounter_state = encounter_state.lock().unwrap();
                let previous_remote_id = encounter_state.crowdsource_monster_remote_id.clone();
                let mut snapshot_to_save = None;
                if process_sync_to_me_delta_info(&mut encounter_state, sync_to_me_delta_info).is_none() {
                    warn!("Error processing SyncToMeDeltaInfo.. ignoring.");
                }
                if encounter_state.crowdsource_monster_remote_id != previous_remote_id {
                    snapshot_to_save = snapshot_from_encounter(&encounter_state);
                }
                drop(encounter_state);
                if let Some(snapshot) = snapshot_to_save {
                    if let Err(err) = save_snapshot(&app_handle, &snapshot) {
                        warn!("live_main::start - failed to persist crowdsourced monster snapshot: {err}");
                    }
                }
            }
            packets::opcodes::Pkt::SyncNearDeltaInfo => {
                let sync_near_delta_info =
                    match blueprotobuf::SyncNearDeltaInfo::decode(Bytes::from(data)) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Error decoding SyncNearDeltaInfo.. ignoring: {e}");
                            continue;
                        }
                    };
                let encounter_state = app_handle.state::<EncounterMutex>();
                let mut encounter_state = encounter_state.lock().unwrap();
                let mut snapshot_to_save = None;
                for aoi_sync_delta in sync_near_delta_info.delta_infos {
                    let previous_remote_id = encounter_state.crowdsource_monster_remote_id.clone();
                    if process_aoi_sync_delta(&mut encounter_state, aoi_sync_delta).is_none() {
                        warn!("Error processing SyncToMeDeltaInfo.. ignoring.");
                        continue;
                    }

                    if encounter_state.crowdsource_monster_remote_id != previous_remote_id {
                        snapshot_to_save = snapshot_from_encounter(&encounter_state);
                    }
                }
                drop(encounter_state);
                if let Some(snapshot) = snapshot_to_save {
                    if let Err(err) = save_snapshot(&app_handle, &snapshot) {
                        warn!("live_main::start - failed to persist crowdsourced monster snapshot: {err}");
                    }
                }
            }
        }
    }
}
