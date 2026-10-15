//! Генератор мира для Torxel с использованием 4D FBM шума для бесшовной тороидальной топологии.
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
use noise::{Fbm, OpenSimplex, NoiseFn};

use torxel::voxel::format::{Cell, CHUNK_HEIGHT, CHUNK_SIDE, CELLS_PER_CHUNK};
use torxel::voxel::height_map::column_tops;
use torxel::voxel::materials::ids::{AIR, BEDROCK, STONE, DIRT, GRASS};
use torxel::io::file_format::{ChunkHeader, WorldHeader};

// ═══════════════════════════════════════════════════════════
// Константы генерации (гибкая настройка без магических чисел)
// ═══════════════════════════════════════════════════════════

/// Базовый уровень ландшафта (условное "море", но без заполнения водой)
const BASE_LEVEL: usize = 32;
/// Максимальная высота гор
const MAX_HEIGHT: usize = 512;
/// Масштаб шума для 4D-отображения (влияет на "ширину" и плавность гор)
const NOISE_SCALE: f64 = 1.5;
/// Количество октав для FBM (детализация: 3-6 оптимально)
const FBM_OCTAVES: usize = 4;
/// Стойкость октав (амплитуда каждой следующей)
const FBM_PERSISTENCE: f64 = 0.5;
/// Частота октав (масштаб каждой следующей)
const FBM_LACUNARITY: f64 = 2.0;

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

    let world_seed: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u32;

    let mut header = WorldHeader::new(chunks_x, chunks_z, CHUNK_HEIGHT as u32).with_spawn(5, 3);

    header.with_world_seed(world_seed);
    
    writer.write_all(bytemuck::bytes_of(&header)).expect("Failed to write header");

    // Инициализация генератора шума
    let mut fbm = Fbm::<OpenSimplex>::new(world_seed);
    fbm.octaves = FBM_OCTAVES;
    fbm.persistence = FBM_PERSISTENCE;
    fbm.lacunarity = FBM_LACUNARITY;

    let world_width = (chunks_x as usize) * CHUNK_SIDE;
    let world_depth = (chunks_z as usize) * CHUNK_SIDE;
    let two_pi = std::f64::consts::PI * 2.0;

    for cz in 0..chunks_z {
        for cx in 0..chunks_x {
            let (chunk_data, chunk_header) = generate_chunk(
                cx as i64, 
                cz as i64, 
                world_width, 
                world_depth, 
                two_pi, 
                &fbm
            );
            
            writer.write_all(bytemuck::bytes_of(&chunk_header)).expect("Failed to write chunk header");
            writer.write_all(bytemuck::cast_slice(&chunk_data)).expect("Failed to write chunk data");
        }
    }

    writer.flush().expect("Failed to flush file");
    println!("Chunks written! {:?}", starter.elapsed());

    let sync_start = Instant::now();
    let inner_file = writer.into_inner().expect("Failed to get file from writer");
    inner_file.sync_all().expect("Failed to sync to disk");
    println!("Synced to disk! {:?}", sync_start.elapsed());
}

fn generate_chunk(
    chunk_x: i64, 
    chunk_z: i64, 
    world_width: usize, 
    world_depth: usize, 
    two_pi: f64,
    fbm: &Fbm<OpenSimplex>
) -> (Vec<Cell>, ChunkHeader) {
    let mut cells = vec![Cell::zeroed(); CELLS_PER_CHUNK];
    
    // Формально обновляем для валидности заголовка, хотя основная логика опирается на column_tops
    let mut min_solid = CHUNK_HEIGHT as u16;
    let mut max_solid = 0u16;

    for lz in 0..CHUNK_SIDE {
        for lx in 0..CHUNK_SIDE {
            // Глобальные координаты блока
            let gx = (chunk_x * CHUNK_SIDE as i64 + lx as i64) as f64;
            let gz = (chunk_z * CHUNK_SIDE as i64 + lz as i64) as f64;

            // 4D-отображение для идеальной тороидальной бесшовности
            let u = (gx / world_width as f64) * two_pi;
            let v = (gz / world_depth as f64) * two_pi;

            let nx = u.cos() * NOISE_SCALE;
            let ny = u.sin() * NOISE_SCALE;
            let nz = v.cos() * NOISE_SCALE;
            let nw = v.sin() * NOISE_SCALE;

            // Получаем значение шума в диапазоне [-1.0, 1.0] и нормализуем в [0.0, 1.0]
            let n = fbm.get([nx, ny, nz, nw]);
            let n_norm = (n + 1.0) / 2.0;

            // Вычисляем высоту ландшафта
            let height_range = (MAX_HEIGHT/2 - BASE_LEVEL) as f64;
            let terrain_height = BASE_LEVEL + (n_norm * height_range) as usize;
            let terrain_height = terrain_height.min(MAX_HEIGHT);

            for ly in 0..CHUNK_HEIGHT {
                let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

                let material = if ly == 0 {
                    BEDROCK
                } else if ly == terrain_height {
                    GRASS
                } else if ly > terrain_height {
                    AIR
                } else if terrain_height - ly < 4 {
                    DIRT // Верхний слой под травой
                } else {
                    STONE
                };

                cells[cell_index].material_id = material;

                if material != AIR {
                    let y = ly as u16;
                    if y < min_solid { min_solid = y; }
                    if y > max_solid { max_solid = y; }
                }
            }
        }
    }

    let chunk_data_size = (CELLS_PER_CHUNK * std::mem::size_of::<Cell>()) as u32;
    let mut header = ChunkHeader::new(chunk_data_size, CELLS_PER_CHUNK as u32);
    
    // Заполняем для совместимости, но основным источником истины является column_tops
    header.min_solid_y = min_solid;
    header.max_solid_y = max_solid;
    header.min_liquid_y = CHUNK_HEIGHT as u16; // Нет жидкостей
    header.max_liquid_y = 0u16;

    // Карта высот вычисляется один раз на основе сгенерированных ячеек
    header.column_tops = column_tops(&cells);

    (cells, header)
}