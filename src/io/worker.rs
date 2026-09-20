//! Фоновый воркер для загрузки и сохранения чанков.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use bytemuck::Zeroable;

use crate::voxel::format::{Cell, CELLS_PER_CHUNK};
use super::channels::{
    LoadRequest, LoadResponse, SaveRequest, SaveResponse,
    ShadowLoadRequest, ShadowLoadResponse, WorkerChannels,
};
use super::file_format::{ChunkHeader, WorldHeader};

/// Фоновый воркер I/O
pub struct IoWorker {
    handle: Option<JoinHandle<()>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl IoWorker {
    pub fn spawn(
        channels: WorkerChannels,
        world_path: PathBuf,
        header: WorldHeader,
    ) -> Self {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&shutdown_flag);

        let handle = thread::spawn(move || {
            Self::worker_loop(channels, world_path, header, flag_clone);
        });

        Self {
            handle: Some(handle),
            shutdown_flag,
        }
    }

    pub fn shutdown(mut self) {
        self.shutdown_flag.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.join().ok();
        }
    }

    fn worker_loop(
        channels: WorkerChannels,
        world_path: PathBuf,
        header: WorldHeader,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut file = match File::options().read(true).write(true).open(&world_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open world file: {}", e);
                return;
            }
        };

        let header_size = std::mem::size_of::<WorldHeader>() as u64;
        let chunk_total_size = header.chunk_total_size();

        loop {
            if shutdown.load(Ordering::Acquire) {
                break;
            }

            // Приоритет 1: активные запросы на загрузку
            while let Ok(request) = channels.load_receiver.try_recv() {
                let response = Self::handle_load_request(
                    &mut file, &header, header_size, chunk_total_size, request,
                );
                channels.load_response_sender.send(response).ok();
            }

            // Приоритет 2: активные запросы на сохранение
            while let Ok(request) = channels.save_receiver.try_recv() {
                let response = Self::handle_save_request(
                    &mut file, &header, header_size, chunk_total_size, request,
                );
                channels.save_response_sender.send(response).ok();
            }

            // Приоритет 3: теневые запросы на загрузку
            while let Ok(request) = channels.shadow_load_receiver.try_recv() {
                let response = Self::handle_shadow_load_request(
                    &mut file, &header, header_size, chunk_total_size, request,
                );
                channels.shadow_load_response_sender.send(response).ok();
            }

            thread::sleep(std::time::Duration::from_millis(1));
        }

        file.flush().ok();
    }

    fn handle_load_request(
        file: &mut File,
        header: &WorldHeader,
        header_size: u64,
        chunk_total_size: u64,
        request: LoadRequest,
    ) -> LoadResponse {
        let chunk_index =
            (request.chunk_z * header.chunks_x as usize + request.chunk_x) as u64;
        let offset = header_size + chunk_index * chunk_total_size;

        // ОТЛАДКА
        println!("[WORKER] Load request: chunk ({},{}), index={}, offset={}",
            request.chunk_x, request.chunk_z, chunk_index, offset);

        if file.seek(SeekFrom::Start(offset)).is_err() {
            return Self::empty_load_response(request.slot);
        }

        let mut chunk_header_bytes = [0u8; 32];
        if file.read_exact(&mut chunk_header_bytes).is_err() {
            return Self::empty_load_response(request.slot);
        }

        let chunk_header: &ChunkHeader = bytemuck::from_bytes(&chunk_header_bytes);

        if !chunk_header.is_valid() {
            return Self::empty_load_response(request.slot);
        }

        let mut data = vec![Cell::zeroed(); CELLS_PER_CHUNK];
        let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut data);

        match file.read_exact(bytes) {
            Ok(_) => LoadResponse {
                slot: request.slot,
                data,
                min_solid_y: chunk_header.min_solid_y,
                max_solid_y: chunk_header.max_solid_y,
                min_liquid_y: chunk_header.min_liquid_y,
                max_liquid_y: chunk_header.max_liquid_y,
                success: true,
            },
            Err(_) => Self::empty_load_response(request.slot),
        }
    }

    fn handle_shadow_load_request(
        file: &mut File,
        header: &WorldHeader,
        header_size: u64,
        chunk_total_size: u64,
        request: ShadowLoadRequest,
    ) -> ShadowLoadResponse {
        let chunk_index =
            (request.chunk_z * header.chunks_x as usize + request.chunk_x) as u64;
        let offset = header_size + chunk_index * chunk_total_size;

        if file.seek(SeekFrom::Start(offset)).is_err() {
            return ShadowLoadResponse {
                slot: request.slot,
                chunk_x: request.chunk_x as i64,
                chunk_z: request.chunk_z as i64,
                data: vec![Cell::zeroed(); CELLS_PER_CHUNK],
                success: false,
            };
        }

        // Пропускаем заголовок чанка (32 байта)
        let mut chunk_header_bytes = [0u8; 32];
        if file.read_exact(&mut chunk_header_bytes).is_err() {
            return ShadowLoadResponse {
                slot: request.slot,
                chunk_x: request.chunk_x as i64,
                chunk_z: request.chunk_z as i64,
                data: vec![Cell::zeroed(); CELLS_PER_CHUNK],
                success: false,
            };
        }

        let chunk_header: &ChunkHeader = bytemuck::from_bytes(&chunk_header_bytes);
        if !chunk_header.is_valid() {
            return ShadowLoadResponse {
                slot: request.slot,
                chunk_x: request.chunk_x as i64,
                chunk_z: request.chunk_z as i64,
                data: vec![Cell::zeroed(); CELLS_PER_CHUNK],
                success: false,
            };
        }

        let mut data = vec![Cell::zeroed(); CELLS_PER_CHUNK];
        let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut data);

        match file.read_exact(bytes) {
            Ok(_) => ShadowLoadResponse {
                slot: request.slot,
                chunk_x: request.chunk_x as i64,
                chunk_z: request.chunk_z as i64,
                data,
                success: true,
            },
            Err(_) => ShadowLoadResponse {
                slot: request.slot,
                chunk_x: request.chunk_x as i64,
                chunk_z: request.chunk_z as i64,
                data: vec![Cell::zeroed(); CELLS_PER_CHUNK],
                success: false,
            },
        }
    }

    fn handle_save_request(
        file: &mut File,
        header: &WorldHeader,
        header_size: u64,
        chunk_total_size: u64,
        request: SaveRequest,
    ) -> SaveResponse {
        let chunk_index =
            (request.chunk_z * header.chunks_x as usize + request.chunk_x) as u64;
        let offset = header_size + chunk_index * chunk_total_size;

        if file.seek(SeekFrom::Start(offset)).is_err() {
            return SaveResponse { slot: request.slot, success: false };
        }

        let chunk_header = ChunkHeader::new(
            (CELLS_PER_CHUNK * std::mem::size_of::<Cell>()) as u32,
            CELLS_PER_CHUNK as u32,
        );
        let header_bytes: &[u8] = bytemuck::bytes_of(&chunk_header);
        if file.write_all(header_bytes).is_err() {
            return SaveResponse { slot: request.slot, success: false };
        }

        let bytes: &[u8] = bytemuck::cast_slice(&request.data);
        match file.write_all(bytes) {
            Ok(_) => SaveResponse { slot: request.slot, success: true },
            Err(_) => SaveResponse { slot: request.slot, success: false },
        }
    }

    fn empty_load_response(slot: usize) -> LoadResponse {
        LoadResponse {
            slot,
            data: vec![Cell::zeroed(); CELLS_PER_CHUNK],
            min_solid_y: 0,
            max_solid_y: 0,
            min_liquid_y: 0,
            max_liquid_y: 0,
            success: false,
        }
    }
}