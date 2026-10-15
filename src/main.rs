use bevy::prelude::*;
use bevy::window::PresentMode;
use torxel::voxel::topology::TorusTopology;
use torxel::TorxelPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                // VSync выключен (2026-10-14): `present` больше не ждёт vblank.
                // Дефолтный `AutoVsync` давал пики кадра, невидимые нашим системам —
                // `frame max` 19–40 мс при `GameFlowWall max` 4.5–9.9.
                // Вернуть `PresentMode::AutoVsync`, если нужен ограничитель FPS.
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TorxelPlugin {
            topology: TorusTopology::new(60, 60),
            world_path: std::path::PathBuf::from("world.bin"),
        })
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    // Ортогональная камера для изометрического вида
    commands.spawn((
        Camera3d::default(),
        // MSAA 4× (дефолт Bevy) — resolve-проход в рендер-графе, вне расписания
        // Update. В 0.20 `Msaa` — компонент камеры, а не ресурс.
        // Вернуть `Msaa::Sample4` (или снять компонент), если понадобится сглаживание.
        Msaa::Off,
    //    Projection::Orthographic(OrthographicProjection {
    //         scale: BASE_ORTHO_SCALE,
    //         ..OrthographicProjection::default_3d()
    //     }),
        Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    
     // Направленный свет (солнце)
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadow_maps_enabled: true,
            ..Default::default()
        },
        Transform::from_xyz(0.0, 100.0, 0.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Фон (небо)
    commands.insert_resource(ClearColor(Color::srgb(0.5, 0.7, 0.9)));
}