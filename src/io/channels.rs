//! Каналы связи для фонового I/O.
use bevy::prelude::*;
use flume::{Receiver, Sender};

use crate::voxel::format::Cell;

/// Запрос на загрузку чанка (активный пул)
#[derive(Clone, Debug)]
pub struct LoadRequest {
    pub slot: usize,
    pub chunk_x: usize,
    pub chunk_z: usize,
    pub priority: u32,
}

/// Ответ на загрузку чанка (активный пул)
#[derive(Clone, Debug)]
pub struct LoadResponse {
    pub slot: usize,
    pub data: Vec<Cell>,
    pub min_solid_y: u16,
    pub max_solid_y: u16,
    pub min_liquid_y: u16,
    pub max_liquid_y: u16,
    pub success: bool,
}

/// Запрос на сохранение чанка (активный пул)
#[derive(Clone, Debug)]
pub struct SaveRequest {
    pub slot: usize,
    pub chunk_x: usize,
    pub chunk_z: usize,
    pub data: Vec<Cell>,
}

/// Ответ на сохранение чанка (активный пул)
#[derive(Clone, Debug)]
pub struct SaveResponse {
    pub slot: usize,
    pub success: bool,
}

/// Запрос на загрузку теневого чанка
#[derive(Clone, Debug)]
pub struct ShadowLoadRequest {
    pub slot: usize,
    pub chunk_x: usize,
    pub chunk_z: usize,
}

/// Ответ на загрузку теневого чанка
#[derive(Clone, Debug)]
pub struct ShadowLoadResponse {
    pub slot: usize,
    pub chunk_x: i64,
    pub chunk_z: i64,
    pub data: Vec<Cell>,
    pub success: bool,
}

/// Менеджер каналов I/O
#[derive(Resource)]
pub struct IoManager {
    // Активные каналы
    pub load_sender: Sender<LoadRequest>,
    pub load_response_receiver: Receiver<LoadResponse>,
    pub save_sender: Sender<SaveRequest>,
    pub save_response_receiver: Receiver<SaveResponse>,

    // Теневые каналы
    pub shadow_load_sender: Sender<ShadowLoadRequest>,
    pub shadow_load_response_receiver: Receiver<ShadowLoadResponse>,
}

impl IoManager {
    pub fn new() -> (Self, WorkerChannels) {
        let (load_sender, load_receiver) = flume::unbounded();
        let (load_response_sender, load_response_receiver) = flume::unbounded();
        let (save_sender, save_receiver) = flume::unbounded();
        let (save_response_sender, save_response_receiver) = flume::unbounded();
        let (shadow_load_sender, shadow_load_receiver) = flume::unbounded();
        let (shadow_load_response_sender, shadow_load_response_receiver) = flume::unbounded();

        let manager = Self {
            load_sender,
            load_response_receiver,
            save_sender,
            save_response_receiver,
            shadow_load_sender,
            shadow_load_response_receiver,
        };

        let worker_channels = WorkerChannels {
            load_receiver,
            load_response_sender,
            save_receiver,
            save_response_sender,
            shadow_load_receiver,
            shadow_load_response_sender,
        };

        (manager, worker_channels)
    }

    pub fn queue_load(&self, request: LoadRequest) {
        self.load_sender.send(request).ok();
    }

    pub fn queue_save(&self, request: SaveRequest) {
        self.save_sender.send(request).ok();
    }

    pub fn queue_shadow_load(&self, request: ShadowLoadRequest) {
        self.shadow_load_sender.send(request).ok();
    }

    pub fn try_recv_load_response(&self) -> Option<LoadResponse> {
        self.load_response_receiver.try_recv().ok()
    }

    pub fn try_recv_save_response(&self) -> Option<SaveResponse> {
        self.save_response_receiver.try_recv().ok()
    }

    pub fn try_recv_shadow_load_response(&self) -> Option<ShadowLoadResponse> {
        self.shadow_load_response_receiver.try_recv().ok()
    }
}

/// Каналы для фонового воркера
pub struct WorkerChannels {
    pub load_receiver: Receiver<LoadRequest>,
    pub load_response_sender: Sender<LoadResponse>,
    pub save_receiver: Receiver<SaveRequest>,
    pub save_response_sender: Sender<SaveResponse>,
    pub shadow_load_receiver: Receiver<ShadowLoadRequest>,
    pub shadow_load_response_sender: Sender<ShadowLoadResponse>,
}