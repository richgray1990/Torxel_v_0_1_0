//! Реестр материалов.

pub mod ids {
    pub const AIR: u8 = 0;
    pub const BEDROCK: u8 = 1;
    pub const STONE: u8 = 2;
    pub const DIRT: u8 = 3;
    pub const GRASS: u8 = 4;
    pub const WATER: u8 = 5;
    pub const CLAY: u8 = 6;
}

pub mod phases {
    pub const SOLID: u8 = 0;
    pub const LIQUID: u8 = 1;
    pub const GAS: u8 = 2;
    pub const PLASMA: u8 = 3;
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub id: u8,
    pub name: &'static str,
    pub color: [u8; 3],
    pub opacity: f32,
    pub phase: u8,
}

pub const MATERIALS: &[Material] = &[
    Material { id: ids::AIR, name: "Air", color: [0, 0, 0], opacity: 0.0, phase: phases::GAS },
    Material { id: ids::BEDROCK, name: "Bedrock", color: [30, 30, 30], opacity: 1.0, phase: phases::SOLID },
    Material { id: ids::STONE, name: "Stone", color: [128, 128, 128], opacity: 1.0, phase: phases::SOLID },
    Material { id: ids::DIRT, name: "Dirt", color: [139, 90, 43], opacity: 1.0, phase: phases::SOLID },
    Material { id: ids::GRASS, name: "Grass", color: [34, 139, 34], opacity: 1.0, phase: phases::SOLID },
    Material { id: ids::WATER, name: "Water", color: [30, 100, 200], opacity: 0.7, phase: phases::LIQUID },
    Material { id: ids::CLAY, name: "Clay", color: [180, 130, 100], opacity: 1.0, phase: phases::SOLID },
];

#[inline]
pub fn get_material(id: u8) -> Option<&'static Material> {
    MATERIALS.iter().find(|m| m.id == id)
}

#[inline]
pub fn material_color(id: u8) -> [u8; 3] {
    get_material(id).map(|m| m.color).unwrap_or([0, 0, 0])
}

#[inline]
pub fn is_transparent(id: u8) -> bool {
    get_material(id).map(|m| m.opacity < 1.0).unwrap_or(true)
}

#[inline]
pub fn is_air(id: u8) -> bool {
    id == ids::AIR
}