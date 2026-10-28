use bevy::diagnostic::{
    EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::window::PresentMode;
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
        // Диагностики Bevy — ВРЕМЕННО, для выхода за пределы `GameFlowWall` (2026-10-15).
        // Наш `PerfStats` меряет только `Update`; здесь — то, что снаружи: FPS/время кадра,
        // число сущностей, а `RenderDiagnosticsPlugin` даёт проходы рендер-графа.
        // Регистрировать ПОСЛЕ `DefaultPlugins`: нужен time-plugin.
        //
        // `EntityCountDiagnosticsPlugin` — проверка утечки сущностей. Замерено: счётчик
        // стоит на ~980 без роста, утечки нет (п. 31 инвариантов закрыт).
        //
        // `RenderDiagnosticsPlugin` живёт в `bevy::render::diagnostic`, НЕ в
        // `bevy::diagnostic` — неверный путь делает вид, что плагина нет.
        // Даёт CPU/GPU-время по проходам (shadow pass, main pass) и pipeline statistics.
        // Готового счётчика draw calls в Bevy НЕТ — это ближайший прокси.
        .add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            // Интервал вывода. Метода `with_update_interval` не существует — поле
            // называется `wait_duration`, дефолт ровно `Duration::from_secs(1)`.
            // Печать идёт ВНУТРИ кадра, поэтому интервал поднят: секунда давала
            // слишком много шума в выводе рядом с нашим `[PERF]`.
            LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(5),
                ..default()
            },
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
        ))
        .add_plugins(TorxelPlugin {
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
            shadow_maps_enabled:  false,
            ..Default::default()
        },
        Transform::from_xyz(0.0, 100.0, 0.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Фон (небо)
    commands.insert_resource(ClearColor(Color::srgb(0.5, 0.7, 0.9)));
}