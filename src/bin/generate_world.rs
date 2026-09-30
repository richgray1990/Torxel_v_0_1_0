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

const PYRAMID_HEIGHTS: [[usize; 2]; 2] = [
    [1, 3],  // chunk (0,0)=3, chunk (0,1)=5
    [5, 8],  // chunk (1,0)=1, chunk (1,1)=7
];

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

            //let ph = quarter_pyramid_height(lx, lz, 7, chunk_x, chunk_z);
            let ph = quarter_pyramid_height(lx, lz, 7, chunk_x, chunk_z);
            
            for ly in 0..CHUNK_HEIGHT {
                let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

                // Плоский мир: только stone на y=0, остальное воздух
                let material = if ly == 0 {
                    STONE
                } else if ly as i64 <= ph as i64 {
                    CLAY
                } else {
                    AIR
                };

                // Чанк (0,0): 4 башни в углах
                if chunk_x == 0 && chunk_z == 0 {
                    // // Угол (0,0): 1x1, высота 2
                    set_tower(&mut cells, 0, 0, 1, 1, 2);
                    // // Угол (0,15): 2x2, высота 3
                    set_tower(&mut cells, 0, 15, 2, 2, 3);
                    // // Угол (15,0): 3x3, высота 4
                    set_tower(&mut cells, 15, 0, 3, 3, 4);
                    // // Угол (15,15): 4x4, высота 5
                    set_tower(&mut cells, 15, 15, 4, 4, 5);
                }
                // Чанк (1,1): 4 башни в углах
                else if chunk_x == 0 && chunk_z == 1 {
                    // Угол (0,0): 2x3, высота 6
                    set_tower(&mut cells, 0, 0, 2, 3, 6);
                    // Угол (0,15): 3x2, высота 7
                    set_tower(&mut cells, 0, 15, 3, 2, 7);
                    // Угол (15,0): 1x4, высота 8
                    set_tower(&mut cells, 15, 0, 1, 4, 8);
                    // Угол (15,15): 4x1, высота 9
                    set_tower(&mut cells, 15, 15, 4, 1, 9);
                }
                
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

    // // ОТЛАДКА: дамп углов после генерации
    // if chunk_x >= 0 && chunk_x <= 1 && chunk_z >= 0 && chunk_z <= 1 {
    //     println!("[GEN] Chunk ({},{}) corners:", chunk_x, chunk_z);
    //     for (lx, lz, name) in [(0, 0, "corner(0,0)"), (15, 0, "corner(15,0)"), 
    //                             (0, 15, "corner(0,15)"), (15, 15, "corner(15,15)")] {
    //         print!("  {}: ", name);
    //         for y in 1..8 {
    //             let idx = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + y;
    //             print!("y{}={} ", y, cells[idx].material_id);
    //         }
    //         println!();
    //     }
    // }

    let chunk_data_size = (CELLS_PER_CHUNK * std::mem::size_of::<Cell>()) as u32;
    let mut header = ChunkHeader::new(chunk_data_size, CELLS_PER_CHUNK as u32);
    header.min_solid_y = min_solid;
    header.max_solid_y = max_solid;
    header.min_liquid_y = min_liquid;
    header.max_liquid_y = max_liquid;

    (cells, header)
}

fn set_tower(
    cells: &mut [Cell],
    corner_x: usize,
    corner_z: usize,
    size_x: usize,
    size_z: usize,
    height: usize,
) {
    for dz in 0..size_z {
        for dx in 0..size_x {
            let lx = if corner_x == 0 { dx } else { CHUNK_SIDE - 1 - dx };
            let lz = if corner_z == 0 { dz } else { CHUNK_SIDE - 1 - dz };
            
            for ly in 1..=height {
                if ly < CHUNK_HEIGHT {
                    let idx = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;
                    cells[idx].material_id = CLAY;
                    cells[idx].phase_state = SOLID;
                }
            }
        }
    }
}

/// Высота пирамидки в точке (wx, wz).
/// Пирамидки стоят на УГЛАХ чанков (стык 4 чанков) чтобы
/// проверить межчанковые боковые грани.
fn quarter_pyramid_height(lx: usize, lz: usize, radius: usize, chunk_x: i64, chunk_z: i64) -> usize {
    let new_radius: usize = ((chunk_x + chunk_z).rem_euclid(radius as i64) + 1) as usize;

    // Расстояние до ближайшего угла чанка
    let dx = lx.min(CHUNK_SIDE - 1 - lx);
    let dz = lz.min(CHUNK_SIDE - 1 - lz);
    let d = dx.max(dz);

    if d < new_radius {
        new_radius - d
    } else {
        0
    }
}

//отладочный случай
fn pyramid_height(lx: usize, lz: usize, chunk_x: i64, chunk_z: i64) -> usize {
    if chunk_x > 1 || chunk_z > 1 {
        return 0;
    }
    
    let max_h = PYRAMID_HEIGHTS[chunk_z as usize][chunk_x as usize];
    let radius = max_h;
    
    let dx = lx.min(CHUNK_SIDE - 1 - lx);
    let dz = lz.min(CHUNK_SIDE - 1 - lz);
    let d = dx.max(dz);
    
     if d < radius {max_h - d } else { 0 }
}