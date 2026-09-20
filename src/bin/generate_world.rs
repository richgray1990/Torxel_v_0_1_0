//! Генератор мира для Toxel.
//!
//! Использование:
//! ```bash
//! cargo run --release --bin generate_world -- 50 60 world.bin
//! ```

use std::time::Instant;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use bytemuck::Zeroable;

use torxel::voxel::format::{Cell, CHUNK_HEIGHT, CHUNK_SIDE, CELLS_PER_CHUNK};
use torxel::voxel::materials::ids::{AIR, BEDROCK, STONE, DIRT, GRASS, CLAY};
use torxel::voxel::materials::phases::{SOLID, GAS};
use torxel::io::file_format::{ChunkHeader, WorldHeader};

fn main() {
    let start_total = Instant::now();

    let args: Vec<String> = std::env::args().collect();

    let chunks_x: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50);
    let chunks_z: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let output_path = args
        .get(3)
        .map(PathBuf::from)
        .unwrap_or(PathBuf::from("world.bin"));

    println!("Generating world: {}x{} chunks", chunks_x, chunks_z);
    println!("Output: {}", output_path.display());

    generate_world(chunks_x, chunks_z, &output_path, &start_total);

    println!("World generation complete! {:?}", start_total.elapsed());
}

fn generate_world(chunks_x: u32, chunks_z: u32, path: &PathBuf, starter: &Instant) {
    let file = File::create(path).expect("Failed to create world file");
    let mut writer = BufWriter::with_capacity(1024 * 1024, file);

    // Заголовок мира
    let header = WorldHeader::new(chunks_x, chunks_z, CHUNK_HEIGHT as u32)
        .with_spawn(
            (5) as i32,
            (3) as i32,
        );
    let header_bytes: &[u8] = bytemuck::bytes_of(&header);
    writer.write_all(header_bytes).expect("Failed to write header");

    // Чанки с заголовками
    for cz in 0..chunks_z {
        for cx in 0..chunks_x {
            let (chunk_data, chunk_header) = generate_chunk(cx as i64, cz as i64);
            let chunk_bytes: &[u8] = bytemuck::cast_slice(&chunk_data);

            // Заголовок чанка
            let chunk_header_bytes: &[u8] = bytemuck::bytes_of(&chunk_header);
            writer
                .write_all(chunk_header_bytes)
                .expect("Failed to write chunk header");

            // Данные чанка
            writer
                .write_all(chunk_bytes)
                .expect("Failed to write chunk data");
        }
    }

    writer.flush().expect("Failed to flush file");
    println!("Chunks written! {:?}", starter.elapsed());

    // Принудительная запись на физический диск
    let sync_start = Instant::now();
    let inner_file = writer.into_inner().expect("Failed to get file from writer");
    inner_file.sync_all().expect("Failed to sync to disk");
    println!("Synced to disk! {:?}", sync_start.elapsed());
}

fn generate_chunk(chunk_x: i64, chunk_z: i64) -> (Vec<Cell>, ChunkHeader) {
    let mut cells = vec![Cell::zeroed(); CELLS_PER_CHUNK];
    let mut min_solid = CHUNK_HEIGHT as u16;
    let mut max_solid = 0u16;
    let mut min_liquid = CHUNK_HEIGHT as u16;
    let mut max_liquid = 0u16;

    for lz in 0..CHUNK_SIDE {
        for lx in 0..CHUNK_SIDE {
            for ly in 0..CHUNK_HEIGHT {
                let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

                // Плоский мир: только stone на y=0, остальное воздух
                let material = if ly == 0 {
                    STONE
                } else {
                    AIR
                };

                cells[cell_index].material_id = material;
                cells[cell_index].phase_state = if material == AIR { GAS } else { SOLID };

                if material != AIR {
                    let y = ly as u16;
                    if y < min_solid {
                        min_solid = y;
                    }
                    if y > max_solid {
                        max_solid = y;
                    }
                }
            }
        }
    }

    let chunk_data_size = (CELLS_PER_CHUNK * std::mem::size_of::<Cell>()) as u32;
    let mut header = ChunkHeader::new(chunk_data_size, CELLS_PER_CHUNK as u32);
    header.min_solid_y = min_solid;
    header.max_solid_y = max_solid;
    header.min_liquid_y = min_liquid;
    header.max_liquid_y = max_liquid;

    (cells, header)
}

fn terrain_height(_x: i64, _z: i64) -> usize {
    1
}