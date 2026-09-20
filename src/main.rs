use bevy::prelude::*;
use torxel::voxel::topology::TorusTopology;
use torxel::TorxelPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TorxelPlugin {
            topology: TorusTopology::new(50, 60),
            world_path: std::path::PathBuf::from("world.bin"),
        })
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    // Ортогональная камера для изометрического вида
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}