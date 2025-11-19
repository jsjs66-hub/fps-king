use bevy::{
    prelude::*,
    app::AppExit,
    window::{ExitCondition, PrimaryWindow, WindowRef, PresentMode},
    render::view::RenderLayers,
    render::camera::RenderTarget,
    render::{RenderPlugin, settings::WgpuSettings},
    ui::IsDefaultUiCamera,
    log::LogPlugin,
    winit::WinitSettings,
    ecs::system::ParamSet,
};
use rand::Rng;

mod gameplay;
mod menu;
mod network;
mod room;
mod network_game;

use gameplay::*;
use gameplay::{CameraStateCache, LastRoleState};
use menu::*;
use network::*;
use room::*;
use network_game::*;

// --- 核心常量 ---
const PLAYER_HP: f32 = 100.0;
const ROUND_TIME_SECONDS: f32 = 30.0;
const BULLETS_PER_ROUND: i32 = 3;
const DODGE_COOLDOWN_SECONDS: f32 = 5.0;
const ACTION_DURATION_SECONDS: f32 = 1.0;

const DAMAGE_HEAD: f32 = 100.0;
const DAMAGE_TORSO: f32 = 40.0;
const DAMAGE_LEGS: f32 = 30.0;

// --- 视觉和物理常量（含新增需求相关配置）---
const PLAYER_SIZE: Vec2 = Vec2::new(50.0, 100.0);
const BRICK_COLS: usize = 22; // 砖块列数
const BRICK_ROWS: usize = 10; // 砖块行数
const BRICK_WIDTH: f32 = 40.0; // 单个砖块宽度（保持不变）
const BRICK_HEIGHT: f32 = 31.25; // 单个砖块高度（保持不变）
const WALL_SIZE: Vec2 = Vec2::new(BRICK_WIDTH * BRICK_COLS as f32, BRICK_HEIGHT * BRICK_ROWS as f32); // 墙面大小：22列×10行
const WALL_POSITION: Vec3 = Vec3::new(0.0, 0.0, 0.0);
const DEFENDER_START_POS: Vec3 = Vec3::new(0.0, -40.0, 1.0); // 防守方生成在墙中心开口处
const ATTACKER_START_POS: Vec3 = Vec3::new(0.0, 200.0, 1.0); // 适配新墙体
const BULLET_SIZE: Vec2 = Vec2::new(8.0, 8.0);
const BULLET_SPEED: f32 = 1200.0;
const MUZZLE_FLASH_DURATION: f32 = 0.1;
const PLAYER_MOVE_SPEED: f32 = 300.0;
const AIM_SPEED: f32 = 300.0;
const MAX_AIM_OFFSET: f32 = 500.0; // 适配新墙体瞄准范围
const SIDE_DODGE_DISTANCE: f32 = 30.0;

// 新增：需求相关常量
const CROSSHAIR_DAMAGE_RANGE: f32 = 50.0; // 准心附近墙破坏范围
const DEFENDER_CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 0.0, 1000.0); // 摄像头跟随偏移

// --- 系统集定义已移至 gameplay.rs ---

// --- 状态定义 ---
#[derive(Debug, Clone, Eq, PartialEq, Hash, States, Default)]
pub enum AppState {
    #[default]
    MainMenu,
    LocalMultiplayer, // 本地双人
    NetworkMenu,      // 局域网对战菜单
    CreatingRoom,     // 创建房间（等待页面）
    JoiningRoom,      // 加入房间（输入房间号）
    InRoom,           // 在房间内（等待开始）
    Playing,          // 游戏中
    GameOver,         // 游戏结束
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, States, Default)]
enum RoundState {
    #[default]
    Attacking,
    Switching,
}

// --- 组件定义 ---
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlayerId {
    Player1,
    Player2,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlayerRole {
    Attacker,
    Defender,
}

// --- 游玩系统组件已移至 gameplay.rs ---

// --- UI 组件 ---
#[derive(Component)]
struct HealthText;

#[derive(Component)]
struct BulletIcon {
    index: usize,
}

#[derive(Component)]
struct TimerText;

#[derive(Component, Debug)]
struct PlayerHealthDisplay {
    player_id: PlayerId,
}

#[derive(Component)]
struct AttackerUI; // 标识进攻方视角的UI元素

#[derive(Component)]
struct DefenderUI; // 标识防守方视角的UI元素

#[derive(Component)]
struct DefenderUIRoot; // 标识防守方UI根节点

#[derive(Component)]
struct AttackerUIRoot; // 标识进攻方UI根节点

#[derive(Component)]
struct ActionCooldownText;

// --- 字体资源 ---
#[derive(Resource)]
pub struct FontResource {
    pub font: Handle<Font>,
}

// --- 帧率限制资源 ---
#[derive(Resource, Default)]
struct FrameRateLimit {
    last_frame_time: Option<std::time::Instant>,
}

/// 帧率限制系统：限制为90 FPS（通过sleep控制）
fn limit_frame_rate_system(mut frame_limit: ResMut<FrameRateLimit>) {
    const TARGET_FPS: f64 = 90.0;
    const FRAME_DURATION: std::time::Duration = std::time::Duration::from_nanos((1_000_000_000.0 / TARGET_FPS) as u64);
    
    if let Some(last_time) = frame_limit.last_frame_time {
        let elapsed = last_time.elapsed();
        if elapsed < FRAME_DURATION {
            let sleep_time = FRAME_DURATION - elapsed;
            std::thread::sleep(sleep_time);
        }
    }
    frame_limit.last_frame_time = Some(std::time::Instant::now());
}

// --- UI相机实体ID资源 ---
#[derive(Resource)]
struct UiCameraEntities {
    attacker_ui_camera: Entity,
    defender_ui_camera: Entity,
}

// --- 角色切换时重建游戏实体资源 ---
#[derive(Resource, Default)]
pub struct RecreateGameEntitiesOnRoleSwitch {
    pub should_recreate: bool,
}

// --- 角色切换缓冲时间资源 ---
#[derive(Resource)]
pub struct RoleSwitchCooldown {
    pub timer: Timer,
}

impl Default for RoleSwitchCooldown {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(0.5, TimerMode::Once), // 0.5秒缓冲时间
        }
    }
}

/// 存储破碎墙体的数据（主机和客户端分别存储）
#[derive(Resource, Default)]
pub struct BrokenWallData {
    /// 主机视角的破碎墙体位置（ViewLayer::AttackerView 或 ViewLayer::DefenderView）
    pub host_broken_segments: Vec<Vec2>,
    /// 客户端视角的破碎墙体位置
    pub client_broken_segments: Vec<Vec2>,
}

// --- 游玩系统资源和事件已移至 gameplay.rs ---

/// 清理所有相机（在进入游戏时）- 删除所有旧相机，因为setup_game会创建新的相机
fn cleanup_ui_camera(
    mut commands: Commands,
    camera_query: Query<Entity, With<Camera2d>>,
) {
    // 删除所有旧相机，因为setup_game会创建新的游戏相机和UI相机
    let mut count = 0;
    for entity in camera_query.iter() {
        // 调试输出已禁用: println!("[清理] 删除旧相机（实体ID: {:?}），setup_game会创建新的相机", entity);
        commands.entity(entity).despawn_recursive();
        count += 1;
    }
    // 调试输出已禁用: println!("[清理] 共清理 {} 个旧相机", count);
}

/// 清理游戏实体（进入游戏时调用，清除残留元素）
fn cleanup_game_entities(
    mut commands: Commands,
    // 清理所有游戏实体（保留窗口和相机）
    entities: Query<Entity, (Without<PrimaryWindow>, Without<Camera>)>,
) {
    // 调试输出已禁用: println!("[清理] 开始清理游戏实体（进入游戏前）...");
    let mut count = 0;
    for entity in entities.iter() {
        commands.entity(entity).despawn_recursive();
        count += 1;
    }
    // 调试输出已禁用: println!("[清理] 游戏实体清理完成，共清理 {} 个实体", count);
}

/// 设置UI根节点的UiTargetCamera（暂时不使用，因为UiTargetCamera不在公开API中）
/// 我们依赖IsDefaultUiCamera和RenderLayers来确保UI正确渲染
/// 由于我们已经给UI元素设置了RenderLayers（layer 10和layer 11），
/// 并且只给当前玩家的UI相机添加了IsDefaultUiCamera，UI应该能正确渲染
fn setup_ui_target_cameras(
    _commands: Commands,
    _ui_camera_entities: Option<Res<UiCameraEntities>>,
    _attacker_ui_root_query: Query<Entity, With<AttackerUIRoot>>,
    _defender_ui_root_query: Query<Entity, With<DefenderUIRoot>>,
) {
    // 暂时不使用UiTargetCamera，因为它在bevy::ui的公开API中不可用
    // 我们依赖IsDefaultUiCamera和RenderLayers来确保UI正确渲染
}

/// 清理游戏资源（退出游戏时调用）
/// 注意：保留相机和UI实体，只清理游戏相关的实体
fn cleanup_game(
    mut commands: Commands,
    // 清理所有游戏实体（保留窗口、相机和UI节点）
    // 注意：保留所有 Camera 和 Node 组件，确保主菜单可以正常渲染
    entities: Query<Entity, (Without<PrimaryWindow>, Without<Camera>, Without<Node>)>,
    network_manager: Option<ResMut<network_game::NetworkManager>>,
    // 确保UI相机存在（如果不存在则创建）
    ui_camera_query: Query<Entity, (With<Camera2d>, With<Camera>)>,
) {
    // 调试输出已禁用: println!("[清理] 开始清理上一局游戏实体...");
    let mut count = 0;
    for entity in entities.iter() {
        commands.entity(entity).despawn_recursive();
        count += 1;
    }
    // 调试输出已禁用: println!("[清理] 游戏实体清理完成，共清理 {} 个实体", count);
    
    // 确保至少有一个UI相机存在（用于渲染主菜单）
    if ui_camera_query.is_empty() {
        // 调试输出已禁用: println!("[清理] 未找到UI相机，创建默认UI相机");
        commands.spawn(Camera2dBundle::default());
    }
    
    if let Some(nm) = network_manager {
        network_game::cleanup_network(nm);
        // 调试输出已禁用: println!("[清理] 已清理网络资源");
    }
}

/// 处理应用退出事件，确保网络资源被清理
fn handle_app_exit(
    mut exit_events: EventReader<AppExit>,
    network_manager: Option<ResMut<network_game::NetworkManager>>,
) {
    if exit_events.is_empty() {
        return;
    }
    if let Some(nm) = network_manager {
        network_game::cleanup_network(nm);
        // 调试输出已禁用: println!("[清理] AppExit事件触发，网络资源已清理");
    }
    exit_events.clear();
}

/// BGM播放器标记组件
#[derive(Component)]
struct BackgroundMusic;

/// BGM资源（用于预加载）
#[derive(Resource)]
struct BgmHandle {
    handle: Handle<bevy::audio::AudioSource>,
    loaded: bool,
}

/// 预加载BGM（在Startup阶段）
fn preload_bgm(
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    // 优先尝试OGG格式（更好的兼容性）
    let bgm_path_ogg = "bgm/bgm.ogg";
    let bgm_path_mp3 = "bgm/【对峙2】syndicate（Full Theme）辛迪加（主题曲） - 1.syndicate（Full Theme）辛迪加（主题曲）(Av115520007309595,P1).mp3";
    
    // 检查OGG文件是否存在
    let bgm_path = if std::path::Path::new(&format!("assets/{}", bgm_path_ogg)).exists() {
        // 调试输出已禁用: println!("[BGM] 找到OGG格式音频文件: {}", bgm_path_ogg);
        bgm_path_ogg
    } else {
        // 调试输出已禁用: println!("[BGM] 未找到OGG文件，使用MP3: {}", bgm_path_mp3);
        // 调试输出已禁用: println!("[BGM] 提示: 如果MP3无法播放，请运行 ./convert_audio.sh 转换为OGG格式");
        bgm_path_mp3
    };
    
    // 调试输出已禁用: println!("[BGM] 预加载音频文件: {}", bgm_path);
    let bgm_handle = asset_server.load(bgm_path);
    commands.insert_resource(BgmHandle {
        handle: bgm_handle,
        loaded: false,
    });
}

/// 检查BGM是否加载完成并播放
fn play_background_music(
    asset_server: Res<AssetServer>,
    bgm_handle: Option<ResMut<BgmHandle>>,
    mut commands: Commands,
    bgm_query: Query<Entity, With<BackgroundMusic>>,
    audio_assets: Res<Assets<bevy::audio::AudioSource>>,
) {
    // 检查是否已经有BGM在播放
    if bgm_query.iter().next().is_some() {
        return;
    }
    
    // 检查BGM是否已加载
    if let Some(mut bgm) = bgm_handle {
        // 检查资源是否已加载
        if !bgm.loaded {
            // 检查资源加载状态
            let load_state = asset_server.load_state(&bgm.handle);
            if load_state == bevy::asset::LoadState::Loaded {
                // 验证音频资源是否真的可用
                if audio_assets.get(&bgm.handle).is_none() {
                    eprintln!("[BGM] 错误: 音频文件格式不支持或无法解码");
                    eprintln!("[BGM] 解决方案: 运行以下命令将MP3转换为OGG格式:");
                    eprintln!("[BGM]   ./convert_audio.sh");
                    eprintln!("[BGM] 或者安装ffmpeg后运行:");
                    eprintln!("[BGM]   ffmpeg -i 'assets/bgm/...mp3' -c:a libvorbis -q:a 5 'assets/bgm/bgm.ogg'");
                    eprintln!("[BGM] 然后重启游戏");
                    // 标记为已加载以避免重复错误
                    bgm.loaded = true;
                    return;
                }
                bgm.loaded = true;
                // 调试输出已禁用: println!("[BGM] 音频文件已加载，准备播放");
            } else if load_state == bevy::asset::LoadState::Failed {
                eprintln!("[BGM] 错误: 音频文件加载失败 - 格式可能不支持");
                eprintln!("[BGM] 解决方案: 将MP3转换为OGG格式:");
                eprintln!("[BGM]   ffmpeg -i 'assets/bgm/...mp3' -c:a libvorbis -q:a 5 'assets/bgm/bgm.ogg'");
                return;
            } else {
                // 还在加载中，等待
                return;
            }
        }
        
        // 播放BGM（只在资源确实可用时）
        // 使用 try_spawn 或其他方式避免在音频解码失败时panic
        match audio_assets.get(&bgm.handle) {
            Some(_) => {
                // 资源可用，尝试播放
                // 注意：即使资源存在，解码时仍可能失败，但至少不会在spawn时panic
                let bgm_entity = commands.spawn((
                    AudioBundle {
                        source: bgm.handle.clone(),
                        settings: PlaybackSettings::LOOP,
                        ..default()
                    },
                    BackgroundMusic,
                )).id();
                
                // 调试输出已禁用: println!("[BGM] BGM实体已创建: {:?}", bgm_entity);
                // 调试输出已禁用: println!("[BGM] 开始播放背景音乐（循环模式）");
            }
            None => {
                eprintln!("[BGM] 错误: 无法播放音频，资源不可用");
                eprintln!("[BGM] 这可能是因为音频格式不被支持");
                eprintln!("[BGM] 解决方案: 运行 ./convert_audio.sh 将MP3转换为OGG格式");
            }
        }
    }
}


// --- 游戏主程序 ---
fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins
        .set(LogPlugin {
            level: bevy::log::Level::WARN, // 只显示警告和错误，减少日志开销
            filter: "wgpu=error,bevy_render=warn,bevy_ecs=warn".to_string(),
            ..default()
        })
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "重生之我是赋能哥".into(),
                resolution: (1600.0, 900.0).into(),
                resizable: true,
                present_mode: PresentMode::AutoNoVsync, // 禁用VSync，使用帧率限制
                ..default()
            }),
            close_when_requested: true,
            exit_condition: ExitCondition::OnPrimaryClosed, // 必须显式指定，不能省略
        })
        .set(RenderPlugin {
            render_creation: WgpuSettings {
                power_preference: bevy::render::settings::PowerPreference::HighPerformance, // 高性能模式
                ..default()
            }.into(),
            ..default()
        })
        .set(ImagePlugin::default_nearest()) // 优化：使用最近邻过滤，减少纹理采样开销
    )
    // 性能优化：使用游戏模式（Continuous更新，最高性能）
    .insert_resource(WinitSettings::game())
    // 性能优化：限制帧率为90 FPS（通过FrameRateLimit资源）
    .init_resource::<FrameRateLimit>()
    // 1. 注册自定义事件（解决 panic 核心）
    .add_event::<PlayerHitEvent>()
    .add_event::<GameOverEvent>()
    .init_resource::<gameplay::RematchState>()
    .init_resource::<gameplay::GameOverDelay>()
    .init_resource::<gameplay::UiStateTracker>() // UI状态跟踪资源（用于优化UI更新系统）
    .add_event::<PlayerActionEvent>()
    .add_event::<gameplay::CameraSwitchEvent>()
    .add_event::<room::ReconnectEvent>()
    // 2. 初始化游戏状态（Bevy 0.13 用 init_state，而非 add_state）
    .init_state::<AppState>()
    .init_state::<RoundState>()
    // 3. 插入资源
    .insert_resource(ViewConfig {
        is_attacker_view: true,
        viewport_entity: None,
    })
    .insert_resource(CursorPosition(Vec2::ZERO))
    .insert_resource(CrosshairOffset(Vec2::ZERO))
    .init_resource::<RoomInfo>() // 初始化房间信息
    .init_resource::<NetworkManager>() // 初始化网络管理器
    .init_resource::<RecreateGameEntitiesOnRoleSwitch>() // 初始化角色切换时重建游戏实体资源
    .init_resource::<LastRoleState>() // 初始化角色状态缓存（用于优化性能）
    .init_resource::<CameraStateCache>() // 初始化相机状态缓存（用于优化性能）
    // 4. 配置系统集（确保所有变体存在，只在Playing状态下运行）
    .configure_sets(
            Update,
            (
            GameplaySystems::InputSystems,
            GameplaySystems::ActionSystems,
            GameplaySystems::ViewSystems,
            GameplaySystems::LogicSystems,
            GameplaySystems::UISystems,
            GameplaySystems::EventSystems,
        ).run_if(in_state(AppState::Playing))
    )
    // 5. 添加系统（按正确变体关联）
    .add_systems(Startup, (
        setup_fonts, 
        setup_ui_camera,
        preload_bgm, // 预加载BGM
        check_image_loading_system, // 检查图片加载状态（只运行一次）
    )) // 首先加载字体和UI相机，并预加载BGM
    .add_systems(PreUpdate, limit_frame_rate_system) // 帧率限制系统（90 FPS）
    
    // 主菜单系统（setup_main_menu 在 cleanup_game 之后执行，见下方）
    .add_systems(Update, handle_main_menu_buttons.run_if(in_state(AppState::MainMenu)))
    .add_systems(OnExit(AppState::MainMenu), cleanup_main_menu)
    
    // 本地双人游戏（直接进入游戏状态，清理room_info以确保是本地模式）
    .add_systems(OnEnter(AppState::LocalMultiplayer), |mut app_state: ResMut<NextState<AppState>>, mut room_info: ResMut<RoomInfo>| {
        // 确保room_info不是连接状态，这样setup_game会识别为本地模式
        room_info.is_connected = false;
        room_info.is_host = false;
        room_info.room_code = None;
        app_state.set(AppState::Playing);
    })
    
    // 网络菜单系统
    .add_systems(OnEnter(AppState::NetworkMenu), setup_network_menu)
    .add_systems(Update, handle_network_menu_buttons.run_if(in_state(AppState::NetworkMenu)))
    .add_systems(OnExit(AppState::NetworkMenu), cleanup_network_menu)
    
    // 创建房间系统（使用网络自动发现）
    .add_systems(OnEnter(AppState::CreatingRoom), (setup_creating_room, create_room))
    .add_systems(Update, (
        handle_network_messages.run_if(in_state(AppState::CreatingRoom)),
        update_room_code_display.run_if(in_state(AppState::CreatingRoom)),
        room::update_host_ip_display.run_if(in_state(AppState::CreatingRoom)),
        handle_room_buttons_creating.run_if(in_state(AppState::CreatingRoom)),
    ))
    .add_systems(OnExit(AppState::CreatingRoom), cleanup_room_ui)
    // 注意：不在这里清理网络资源，因为返回按钮已经清理了
    
    // 加入房间系统（自动搜索并加入）
    .init_resource::<room::ReconnectFlag>()
    .add_systems(OnEnter(AppState::JoiningRoom), setup_joining_room_simple)
    .add_systems(Update, (
        room::handle_ip_input_box_click.run_if(in_state(AppState::JoiningRoom)),
        room::handle_ip_keyboard_input.run_if(in_state(AppState::JoiningRoom)),
        room::handle_ip_input_and_connect.run_if(in_state(AppState::JoiningRoom)),
        room::handle_reconnect_event.run_if(in_state(AppState::JoiningRoom)),
        room::execute_reconnect.run_if(in_state(AppState::JoiningRoom)).after(room::handle_reconnect_event),
        // 如果没有手动IP且还没有开始搜索，则自动搜索
        room::auto_search_if_needed.run_if(in_state(AppState::JoiningRoom)),
        handle_network_messages.run_if(in_state(AppState::JoiningRoom)),
    ))
    .add_systems(OnExit(AppState::JoiningRoom), cleanup_room_ui)
    // 注意：不在这里清理网络资源，因为游戏进行中需要socket
    
    // 在房间内系统（等待开始）
    .add_systems(OnEnter(AppState::InRoom), room::setup_in_room)
    .add_systems(Update, (
        handle_network_messages.run_if(in_state(AppState::InRoom)),
        room::handle_room_buttons_in_room.run_if(in_state(AppState::InRoom)),
        room::update_creating_room_status.run_if(in_state(AppState::CreatingRoom)),
    ))
    .add_systems(OnExit(AppState::InRoom), cleanup_room_ui)
    
    // 游戏系统
    .add_systems(OnEnter(AppState::Playing), (
        cleanup_game_entities.before(setup_game), // 先清理所有游戏实体（包括旧相机），再创建新的
        cleanup_ui_camera.before(setup_game), // 清理默认UI相机，在setup_game之前
        cleanup_room_ui, 
        cleanup_network_menu,  // 确保清理网络菜单UI
        cleanup_main_menu,     // 确保清理主菜单UI
        gameplay::preload_sound_effects, // 预加载音效资源
        setup_game, // 创建新的游戏相机和UI相机
        update_network_ui_visibility_once.after(setup_game), // 在UI创建后立即更新一次显示状态
    ))
    // 在Update中检查并播放BGM（等待加载完成）
    .add_systems(Update, play_background_music.run_if(in_state(AppState::Playing)))
    .add_systems(Update, (
        handle_network_messages.run_if(in_state(AppState::Playing)).before(network_game::handle_game_state_system), // 处理网络消息（必须在handle_game_state_system之前）
        // 网络游戏状态同步系统
        network_game::sync_game_state_system.run_if(in_state(AppState::Playing)), // 主机发送游戏状态
        network_game::handle_game_state_system.run_if(in_state(AppState::Playing)), // 客户端接收游戏状态
        network_game::handle_client_role_switch.run_if(in_state(AppState::Playing)).after(network_game::handle_game_state_system), // 客户端处理角色切换
        network_game::sync_player_input_system.run_if(in_state(AppState::Playing)), // 防守方发送防守方状态（角色切换后，房主或客户端都可能发送）
        network_game::handle_player_input_system.run_if(in_state(AppState::Playing)).after(network_game::handle_game_state_system), // 接收防守方状态（在GameState之后，优先更新防守方位置）
        network_game::sync_crosshair_position_system.run_if(in_state(AppState::Playing)), // 进攻方发送准星位置（角色切换后，房主或客户端都可能发送）
        network_game::handle_crosshair_position_system.run_if(in_state(AppState::Playing)).after(network_game::handle_player_input_system), // 防守方接收准星位置（在handle_player_input_system之后，确保消息不被重复处理）
        network_game::handle_bullet_spawn_system.run_if(in_state(AppState::Playing)), // 接收方创建子弹（双方都需要处理）
        network_game::handle_health_update_system.run_if(in_state(AppState::Playing)), // 接收方更新血量（双方都需要处理）
    ))
    .add_systems(Update, (
                attacker_aim_system,
                attacker_shoot_system
                    .after(network_game::handle_player_input_system) // 确保防守方位置已更新
                    .before(check_win_condition_system), // 确保射击在游戏结束检查之前
                defender_move_system,
    ).in_set(GameplaySystems::InputSystems))
    .add_systems(Update, (
        defender_action_system,
        action_timer_system,
    ).in_set(GameplaySystems::ActionSystems))
    .add_systems(Update, (
        // check_image_loading_system, // 检查图片加载状态（已移至Startup，只运行一次）
        recreate_game_entities_on_role_switch_system.after(gameplay::switch_roles_system), // 角色切换后重建游戏实体
        gameplay::ensure_network_view_matches_role_system.before(gameplay::switch_network_camera_system), // 确保视角与角色保持一致
        gameplay::switch_network_camera_system.after(gameplay::switch_roles_system).before(update_crosshair_position_system), // 网络模式相机切换（在角色切换之后立即执行）
        cleanup_camera_components_system
            .before(ensure_single_active_camera_system)
            .run_if(in_state(AppState::Playing))
            .run_if(|view_config: Res<ViewConfig>| view_config.is_changed()), // 清理相机上多余的组件（只在视图配置改变时运行）
        // cleanup_attacker_view_entities_system.after(recreate_game_entities_on_role_switch_system).after(setup_game).run_if(in_state(AppState::Playing)), // 清理进攻方视角中属于 layer 1 的实体（已禁用以提升性能）
        // debug_all_cameras_system.run_if(in_state(AppState::Playing)), // 调试：打印所有相机信息（已禁用以提升性能）
        update_role_switch_cooldown.run_if(in_state(AppState::Playing)), // 更新角色切换缓冲计时器
        ensure_ui_camera_exists_system
            .after(recreate_game_entities_on_role_switch_system)
            .run_if(in_state(AppState::Playing)), // 确保UI相机存在（紧急检查）
        ensure_single_active_camera_system
            .after(gameplay::switch_network_camera_system)
            .after(recreate_game_entities_on_role_switch_system)
            .after(update_role_switch_cooldown)
            .after(ensure_ui_camera_exists_system)
            .run_if(in_state(AppState::Playing)), // 确保只有一个游戏相机和一个UI相机处于激活状态（在相机切换之后运行）
        update_network_ui_visibility
            .run_if(in_state(AppState::Playing).or_else(in_state(AppState::GameOver)))
            .run_if(|view_config: Res<ViewConfig>, player_query: Query<&crate::PlayerRole, (With<crate::PlayerId>, Changed<crate::PlayerRole>)>| {
                view_config.is_changed() || !player_query.is_empty()
            }), // 网络模式UI显示/隐藏（只在视图配置或角色改变时运行）
        force_defender_ui_visible.run_if(in_state(AppState::Playing).or_else(in_state(AppState::GameOver))).after(update_network_ui_visibility), // 强制显示防守方UI（在所有其他系统之后运行）
        // 调试系统已禁用以提升性能
        // debug_ui_camera_and_defender_ui.run_if(in_state(AppState::Playing).or_else(in_state(AppState::GameOver))).after(force_defender_ui_visible), // 调试UI相机和防守方UI的渲染关系
        // debug_defender_ui_children.run_if(in_state(AppState::Playing).or_else(in_state(AppState::GameOver))).after(force_defender_ui_visible), // 调试防守方UI子元素
        update_crosshair_position_system,
        update_defender_crosshair_indicator_system, // 更新防守方视角的准星指示器
        update_viewports.run_if(|room_info: Res<RoomInfo>| !room_info.is_connected), // 只在本地模式下运行，避免与网络模式系统冲突
        update_laser_indicator_system.before(update_laser_visibility_system),
        update_laser_visibility_system,
        update_humanoid_sprite_positions.before(defender_visibility_system), // 确保身体部位位置更新在可见性检测之前
    ).in_set(GameplaySystems::ViewSystems))
    .add_systems(Update, (
                bullet_movement_system,
        muzzle_flash_system,
                collision_detection_system.before(wall_visibility_update_system), // 确保碰撞检测在墙段可见性更新之前
        wall_visibility_update_system, // 墙段可见性更新（破损的墙段在进攻方视角中隐藏）
        defender_visibility_system, // 防守方可见性（简化版本，依赖Z轴顺序和墙段隐藏）
                round_timer_update_system,
                check_win_condition_system,
                gameplay::game_over_delay_system.after(attacker_shoot_system), // 游戏结束延迟系统（在射击系统之后运行）
        delayed_round_switch_system,
        follow_defender_camera_system.after(defender_move_system), // 防守方相机跟随应该在防守方移动之后
    ).in_set(GameplaySystems::LogicSystems))
    .add_systems(Update, (
                update_ui,
        update_health_display,
        update_action_cooldown_display,
    ).in_set(GameplaySystems::UISystems))
    .add_systems(Update, (
        handle_player_hit_event,
        handle_game_over_event,
        handle_player_action_event,
        gameplay::handle_number_key_sound_system.run_if(in_state(AppState::Playing)), // 数字键音效（仅在游戏中）
    ).in_set(GameplaySystems::EventSystems))
    .add_systems(OnEnter(RoundState::Switching), (cleanup_bullets_on_switch, switch_roles_system))
        .add_systems(OnEnter(AppState::GameOver), setup_gameover_screen)
    .add_systems(Update, (
        handle_gameover_input,
        handle_rematch_system,
    ).chain().run_if(in_state(AppState::GameOver)))
    // 在GameOver状态下也更新UI，确保防守方能看到最终状态
    .add_systems(Update, (
        update_ui,
        update_health_display,
        update_action_cooldown_display,
    ).run_if(in_state(AppState::GameOver)))
    // 处理游戏结束网络消息（在Playing和GameOver状态都检查，确保客户端能收到）
    // 必须在 handle_game_state_system 之后运行，确保先处理同步消息，再处理游戏结束消息
    // 注意：不能使用 .after(handle_network_messages)，因为它在多个状态下被注册，会导致排序冲突
    .add_systems(Update, (
        gameplay::handle_game_over_network_system
            .run_if(in_state(AppState::Playing))
            .after(network_game::handle_game_state_system), // 确保先处理同步消息（handle_game_state_system已经在handle_network_messages之后运行）
        gameplay::handle_game_over_network_system.run_if(in_state(AppState::GameOver)),
    ))
    // 注意：不在 OnExit(Playing) 时清理，因为 GameOver 状态仍需要游戏实体
    // 只在真正退出到 MainMenu 时才清理（通过检查上一个状态）
    // 确保清理在设置主菜单之前执行
    .add_systems(OnEnter(AppState::MainMenu), (
        cleanup_game.before(setup_main_menu),
        setup_main_menu,
    ));

    app.add_systems(PostUpdate, handle_app_exit);
    
    app.run();
}

// --- 系统实现 ---
/// 创建人形sprite（头部、躯干、腿部）
/// 在网络模式下，只为当前玩家创建需要的实体副本
/// 在本地模式下，为每个玩家创建两个副本（进攻方视角和防守方视角）
fn spawn_humanoid_sprite(
    commands: &mut Commands,
    player_id: PlayerId,
    color: Color,
    position: Vec3,
    head_image: Option<Handle<Image>>,
    is_network_mode: bool,
    is_current_player: bool,
    current_player_is_attacker: bool,
) {
    let player_height = PLAYER_SIZE.y;
    let player_width = PLAYER_SIZE.x;
    
    // 确定要创建的视角
    let view_layers_to_create = if is_network_mode {
        // 网络模式：只为所有玩家创建当前玩家视角的实体副本
        // 如果当前玩家是进攻方，所有玩家都只创建layer 0的实体
        // 如果当前玩家是防守方，所有玩家都只创建layer 1的实体
        if current_player_is_attacker {
            vec![ViewLayer::AttackerView] // 进攻方视角：所有玩家都创建layer 0的实体
        } else {
            vec![ViewLayer::DefenderView] // 防守方视角：所有玩家都创建layer 1的实体
        }
    } else {
        // 本地模式：创建两个视角的副本
        vec![ViewLayer::AttackerView, ViewLayer::DefenderView]
    };
    
        // 为每个部位创建需要的sprite副本
        for view_layer in view_layers_to_create {
            let render_layer = match view_layer {
                ViewLayer::AttackerView => RenderLayers::layer(0), // 进攻方视角渲染层
                ViewLayer::DefenderView => RenderLayers::layer(1), // 防守方视角渲染层
            };
            
            // 根据视角设置不同的Z轴
            let z_pos = match view_layer {
                ViewLayer::AttackerView => 1.0, // 进攻方视角：人物在Z轴1.0（先渲染）
                ViewLayer::DefenderView => 2.0, // 防守方视角：人物在Z轴2.0（后渲染，在墙之后）
            };
            
            // 头部：占身高的30%，位于顶部
            let head_height = player_height * 0.3;
            let head_y = position.y + (player_height - head_height) / 2.0;
            let head_size = Vec2::new(player_width * 0.8, head_height);
            
            // 如果提供了图片，使用图片；否则使用纯色
            let head_bundle = if let Some(ref image_handle) = head_image {
                // 调试输出已禁用: println!("创建头部sprite，使用图片，玩家={:?}, 视角={:?}, 尺寸={:?}", player_id, view_layer, head_size);
                // 使用 with_image 方法或者直接设置 texture 字段
                let mut bundle = SpriteBundle {
            sprite: Sprite {
                        custom_size: Some(head_size),
                ..default()
            },
                    transform: Transform::from_translation(Vec3::new(position.x, head_y, z_pos)),
                    visibility: Visibility::Visible,
            ..default()
                };
                // 设置图片纹理
                bundle.texture = image_handle.clone();
                bundle
            } else {
        SpriteBundle {
            sprite: Sprite {
                        color: color * 0.9, // 头部稍微暗一点
                        custom_size: Some(head_size),
                ..default()
            },
                    transform: Transform::from_translation(Vec3::new(position.x, head_y, z_pos)),
                    visibility: Visibility::Visible,
            ..default()
                }
            };
            
            commands.spawn((
                HumanoidPart {
                    player_id,
                    part_type: HumanoidPartType::Head,
                    view_layer,
                },
                head_bundle,
                render_layer,
            ));
            
            // 躯干：占身高的50%，位于中间
            let torso_height = player_height * 0.5;
            let torso_y = position.y - (player_height - torso_height) / 2.0 + head_height / 2.0;
    commands.spawn((
                HumanoidPart {
                    player_id,
                    part_type: HumanoidPartType::Torso,
                    view_layer,
                },
        SpriteBundle {
            sprite: Sprite {
                        color: color,
                        custom_size: Some(Vec2::new(player_width * 0.9, torso_height)),
                ..default()
            },
                    transform: Transform::from_translation(Vec3::new(position.x, torso_y, z_pos)),
                    visibility: Visibility::Visible,
            ..default()
        },
                render_layer,
    ));

            // 腿部：占身高的20%，位于底部
            let legs_height = player_height * 0.2;
            let legs_y = position.y - (player_height - legs_height) / 2.0;
    commands.spawn((
                HumanoidPart {
                    player_id,
                    part_type: HumanoidPartType::Legs,
                    view_layer,
                },
        SpriteBundle {
            sprite: Sprite {
                        color: color * 0.8, // 腿部稍微暗一点
                        custom_size: Some(Vec2::new(player_width * 0.7, legs_height)),
                ..default()
            },
                    transform: Transform::from_translation(Vec3::new(position.x, legs_y, z_pos)),
                    visibility: Visibility::Visible,
            ..default()
        },
                render_layer,
            ));
        }
}

/// 设置UI相机（用于渲染UI）
fn setup_ui_camera(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

/// 初始化字体（尝试加载中文字体，如果失败则使用默认字体）
fn setup_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    // 尝试加载中文字体文件
    // 用户需要将中文字体文件放在 assets/fonts/ 目录下
    // 推荐使用 NotoSansCJK 或 SourceHanSans 等开源字体
    // 可以从以下地址下载：
    // - Noto Sans CJK: https://www.google.com/get/noto/
    // - Source Han Sans: https://github.com/adobe-fonts/source-han-sans
    
    // 优先使用微软雅黑字体
    let font_paths = [
        "fonts/微软雅黑.ttf", // 用户提供的微软雅黑字体
        "Noto_Sans_SC/NotoSansSC-VariableFont_wght.ttf",
        "fonts/NotoSansCJK-Regular.ttf",
        "fonts/SourceHanSansCN-Regular.otf", 
        "fonts/NotoSansSC-Regular.otf",
        "fonts/simsun.ttf",
        "fonts/msyh.ttf",
    ];
    
    // 直接加载第一个字体（微软雅黑）
    // 注意：即使文件不存在，asset_server.load也不会panic，只是会使用默认字体
    let font: Handle<Font> = asset_server.load(font_paths[0]);
    
    // 调试输出已禁用: println!("正在加载字体: {} (如果文件不存在，将使用默认字体)", font_paths[0]);
    // 调试输出已禁用: println!("提示: 中文字体文件位于 assets/fonts/ 目录");
    
    commands.insert_resource(FontResource { font });
}

/// 初始化游戏
fn setup_game(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    _view_config: Res<ViewConfig>,
    font_resource: Res<FontResource>,
    room_info: Res<RoomInfo>,
    broken_wall_data: Option<Res<BrokenWallData>>,
) {
    // 调试输出已禁用: println!("=== 开始设置游戏 ===");
    println!("[调试] RoomInfo状态: is_connected={}, is_host={}, room_code={:?}", 
             room_info.is_connected, room_info.is_host, room_info.room_code);
    
    // 初始化破碎墙体数据资源（如果不存在）
    if broken_wall_data.is_none() {
        commands.insert_resource(BrokenWallData::default());
        // 调试输出已禁用: println!("[破碎墙体] 初始化BrokenWallData资源");
    }
    
    // 初始化角色切换缓冲计时器资源（如果不存在）
    commands.insert_resource(RoleSwitchCooldown::default());
    
    // 获取字体
    let font = font_resource.font.clone();
    
    // 判断是否为网络对战模式
    let is_network_mode = room_info.is_connected;
    // 调试输出已禁用: println!("[调试] 游戏模式: {}", if is_network_mode { "网络模式" } else { "本地模式" });
    // 调试输出已禁用: println!("[调试] 网络模式下，房主={}, 将创建游戏实体", room_info.is_host);
    
    // 决定初始角色
    let (p1_role, p2_role, current_attacker) = if is_network_mode {
        // 网络模式：房主是进攻方（Player1），加入者是防守方（Player2）
        let is_host = room_info.is_host;
        // 调试输出已禁用: println!("[网络模式] 房主: {}, 分配角色", is_host);
        if is_host {
            // 房主：Player1 = Attacker（进攻方）
            (PlayerRole::Attacker, PlayerRole::Defender, PlayerId::Player1)
        } else {
            // 客户端：Player2 = Defender（防守方），但P1仍然是Attacker（由房主控制）
            (PlayerRole::Attacker, PlayerRole::Defender, PlayerId::Player1)
        }
    } else {
        // 本地模式：随机决定
    let p1_starts_as_attacker = rand::thread_rng().gen_bool(0.5);
        // 调试输出已禁用: println!("[本地模式] 随机分配角色: P1作为{}", if p1_starts_as_attacker { "进攻方" } else { "防守方" });
        if p1_starts_as_attacker {
        (PlayerRole::Attacker, PlayerRole::Defender, PlayerId::Player1)
    } else {
        (PlayerRole::Defender, PlayerRole::Attacker, PlayerId::Player2)
        }
    };
    // 调试输出已禁用: println!("[调试] 最终角色分配: P1={:?}, P2={:?}, 当前进攻方={:?}", p1_role, p2_role, current_attacker);
    
    commands.insert_resource(RoundInfo {
        bullets_left: BULLETS_PER_ROUND,
        round_timer: Timer::from_seconds(ROUND_TIME_SECONDS, TimerMode::Once),
        current_attacker,
        p1_health: PLAYER_HP,
        p2_health: PLAYER_HP,
        bullets_fired_this_round: 0,
        bullets_hit_defender: 0,
        is_switching: false,
    });
    
    // 初始化子弹ID计数器
    commands.insert_resource(gameplay::BulletIdCounter::default());

    // 根据游戏模式创建相机
    if is_network_mode {
        // 网络模式：只创建当前玩家的相机（单窗口显示）
        // 视图配置应该基于当前玩家的角色，而不是基于玩家身份
        let is_host = room_info.is_host;
        // 确定当前玩家的角色：房主是Player1，客户端是Player2
        let current_player_role = if is_host {
            p1_role // 房主是Player1
        } else {
            p2_role // 客户端是Player2
        };
        // 基于角色来决定视图配置，而不是基于玩家身份
        let is_current_attacker = matches!(current_player_role, PlayerRole::Attacker);
        
        // 网络模式下，只创建当前玩家需要的游戏相机
        let camera = if is_current_attacker {
            // 进攻方：只创建进攻方相机（layer 0）
            // 同时作为UI相机，这样UI会直接渲染到游戏相机，不会"捕捉"现有画面
            let attacker_camera_entity = commands.spawn((
                Camera2dBundle {
                    transform: Transform::from_translation(Vec3::new(ATTACKER_START_POS.x, ATTACKER_START_POS.y, 1000.0)),
                    projection: OrthographicProjection {
                        scale: 0.5,
                        ..default()
                    }.into(),
                    camera: Camera {
                        order: 0,
                        clear_color: ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0)),
                        viewport: None, // 网络模式下不设置viewport，使用默认全屏
                        is_active: true, // 进攻方相机始终激活
                        ..default()
                    },
                    ..default()
                },
                RenderLayers::layer(0),
                IsDefaultUiCamera, // 让游戏相机同时作为UI相机，UI直接渲染到游戏相机，不会"捕捉"现有画面
            )).id();
            // 调试输出已禁用: println!("[网络模式] 创建进攻方相机（基于角色：进攻方），实体ID: {:?}, is_active=true, RenderLayers::layer(0), 同时作为UI相机", attacker_camera_entity);
            attacker_camera_entity
        } else {
            // 防守方：只创建防守方相机（layer 1）
            let defender_camera_entity = commands.spawn((
                DefenderCamera,
                Camera2dBundle {
                    transform: Transform::from_translation(Vec3::new(WALL_POSITION.x, WALL_POSITION.y, 1000.0)),
                    projection: OrthographicProjection {
                        scale: 1.5,
                        ..default()
                    }.into(),
                    camera: Camera {
                        order: 0,
                        clear_color: ClearColorConfig::Custom(Color::rgb(0.2, 0.0, 0.2)), // 使用明显的紫色背景，便于测试（RGB: 0.2, 0.0, 0.2）
                        viewport: None, // 网络模式下不设置viewport，使用默认全屏
                        is_active: true, // 防守方相机始终激活
                        ..default()
                    },
                    ..default()
                },
                RenderLayers::layer(1),
                // 不添加IsDefaultUiCamera，让UI自动使用UI相机
            )).id();
            // 调试输出已禁用: println!("[网络模式] 创建防守方相机（基于角色：防守方），实体ID: {:?}, is_active=true, RenderLayers::layer(1)", defender_camera_entity);
            defender_camera_entity
        };
        
        // 调试输出已禁用: println!("[网络模式] 相机实体ID: {:?}", camera);
        
        // 网络模式下，根据角色创建UI相机
        let (attacker_ui_camera_entity, defender_ui_camera_entity) = if is_current_attacker {
            // 进攻方：使用游戏相机作为UI相机，不需要单独的UI相机
            // 这样UI会直接渲染到游戏相机，不会"捕捉"现有画面
            // 调试输出已禁用: println!("[网络模式] 进攻方：使用游戏相机作为UI相机，不创建单独的UI相机");
            (camera, Entity::PLACEHOLDER) // 使用游戏相机实体ID作为UI相机
        } else {
            // 防守方：创建单独的UI相机（保持原状）
            let defender_ui_camera_entity = commands.spawn((
                Camera2dBundle {
                    camera: Camera {
                        order: 1000, // 设置非常高的order，确保UI相机在所有游戏相机之后渲染
                        clear_color: ClearColorConfig::None, // 不清空，叠加在游戏画面上
                        is_active: true, // 防守方UI相机始终激活
                        ..default()
                    },
                    ..default()
                },
                IsDefaultUiCamera, // 添加IsDefaultUiCamera，让UI元素渲染到这个相机
                // 不设置RenderLayers，使用默认的layer 0
            )).id();
            // 调试输出已禁用: println!("[网络模式] 创建防守方UI相机（order: 1000，无RenderLayers，is_active=true），实体ID: {:?}", defender_ui_camera_entity);
            (Entity::PLACEHOLDER, defender_ui_camera_entity) // 进攻方UI相机使用占位符
        };
        
        // 保存UI相机的实体ID，以便后续设置UiTargetCamera
        commands.insert_resource(UiCameraEntities {
            attacker_ui_camera: attacker_ui_camera_entity,
            defender_ui_camera: defender_ui_camera_entity,
        });
        
        commands.insert_resource(ViewConfig {
            is_attacker_view: is_current_attacker,
            viewport_entity: Some(camera),
        });
        
        println!("[网络模式] 当前玩家角色: {:?}, 是房主: {}, 视图配置: {} (基于角色，而不是玩家身份)", 
                 if is_current_attacker { "进攻方" } else { "防守方" }, 
                 is_host, 
                 if is_current_attacker { "进攻方视图" } else { "防守方视图" });
            } else {
        // 本地模式：创建两个相机（双窗口显示）
        // 左侧显示P1视角，右侧显示P2视角
        // 调试输出已禁用: println!("[本地模式] 创建双窗口相机：左侧=P1视角，右侧=P2视角");
        
        // 根据P1的角色决定左侧相机的设置
        let (left_camera_pos, left_camera_scale, left_camera_color, left_render_layer, left_is_defender_camera) = 
            if matches!(p1_role, PlayerRole::Attacker) {
                // P1是进攻方
                (Vec3::new(ATTACKER_START_POS.x, ATTACKER_START_POS.y, 1000.0), 0.5, Color::rgb(0.0, 0.0, 0.0), RenderLayers::layer(0), false)
            } else {
                // P1是防守方
                (DEFENDER_CAMERA_OFFSET, 1.5, Color::rgb(0.05, 0.05, 0.1), RenderLayers::layer(1), true)
            };
        
        // 根据P2的角色决定右侧相机的设置
        let (right_camera_pos, right_camera_scale, right_camera_color, right_render_layer, right_is_defender_camera) = 
            if matches!(p2_role, PlayerRole::Attacker) {
                // P2是进攻方
                (Vec3::new(ATTACKER_START_POS.x, ATTACKER_START_POS.y, 1000.0), 0.5, Color::rgb(0.0, 0.0, 0.0), RenderLayers::layer(0), false)
            } else {
                // P2是防守方
                (DEFENDER_CAMERA_OFFSET, 1.5, Color::rgb(0.05, 0.05, 0.1), RenderLayers::layer(1), true)
            };
        
        // 创建左侧相机（P1视角）
        let left_camera_entity = if left_is_defender_camera {
    commands.spawn((
                DefenderCamera,
                gameplay::PlayerCamera { player_id: PlayerId::Player1 }, // 标记这是P1的相机
                Camera2dBundle {
                    transform: Transform::from_translation(left_camera_pos),
                    projection: OrthographicProjection {
                        scale: left_camera_scale,
                        ..default()
                    }.into(),
                    camera: Camera {
                        order: 0, // 左侧相机
                        clear_color: ClearColorConfig::Custom(left_camera_color),
                        viewport: None, // 不在这里设置viewport，将在update_viewports系统中设置
                ..default()
            },
            ..default()
        },
                left_render_layer,
            )).id()
        } else {
    commands.spawn((
                gameplay::PlayerCamera { player_id: PlayerId::Player1 }, // 标记这是P1的相机
                Camera2dBundle {
                    transform: Transform::from_translation(left_camera_pos),
                    projection: OrthographicProjection {
                        scale: left_camera_scale,
                ..default()
                    }.into(),
                    camera: Camera {
                        order: 0, // 左侧相机
                        clear_color: ClearColorConfig::Custom(left_camera_color),
                        viewport: None, // 不在这里设置viewport，将在update_viewports系统中设置
            ..default()
        },
        ..default()
                },
                left_render_layer,
            )).id()
        };
        
        // 创建右侧相机（P2视角）
        let right_camera_entity = if right_is_defender_camera {
            commands.spawn((
                DefenderCamera,
                gameplay::PlayerCamera { player_id: PlayerId::Player2 }, // 标记这是P2的相机
                Camera2dBundle {
                    transform: Transform::from_translation(right_camera_pos),
                    projection: OrthographicProjection {
                        scale: right_camera_scale,
                        ..default()
                    }.into(),
                    camera: Camera {
                        order: 1, // 右侧相机
                        clear_color: ClearColorConfig::Custom(right_camera_color),
                        viewport: None, // 不在这里设置viewport，将在update_viewports系统中设置
            ..default()
        },
        ..default()
                },
                right_render_layer,
            )).id()
        } else {
            commands.spawn((
                gameplay::PlayerCamera { player_id: PlayerId::Player2 }, // 标记这是P2的相机
                Camera2dBundle {
                    transform: Transform::from_translation(right_camera_pos),
                    projection: OrthographicProjection {
                        scale: right_camera_scale,
                        ..default()
                    }.into(),
                    camera: Camera {
                        order: 1, // 右侧相机
                        clear_color: ClearColorConfig::Custom(right_camera_color),
                        viewport: None, // 不在这里设置viewport，将在update_viewports系统中设置
                ..default()
            },
            ..default()
                },
                right_render_layer,
            )).id()
        };
        
        commands.insert_resource(ViewConfig {
            is_attacker_view: matches!(p1_role, PlayerRole::Attacker), // 左侧是P1视角
            viewport_entity: Some(left_camera_entity),
        });
        
        // 存储相机的实体ID
        commands.insert_resource(gameplay::LocalMultiplayerCameras {
            left_camera: left_camera_entity,
            right_camera: right_camera_entity,
        });
        
        // 调试输出已禁用: println!("[本地模式] 左侧相机：P1视角（{:?}），右侧相机：P2视角（{:?}）", p1_role, p2_role);
    }

    // 创建玩家1（蓝色）
    let p1_pos = if matches!(p1_role, PlayerRole::Attacker) { ATTACKER_START_POS } else { DEFENDER_START_POS };
    commands.spawn((
        PlayerId::Player1,
        p1_role,
        Health(PLAYER_HP),
        Transform::from_translation(p1_pos),
        Visibility::Visible,
        Collider { size: PLAYER_SIZE },
        ActionCooldown {
            last_action_time: 0.0,
            cooldown_duration: DODGE_COOLDOWN_SECONDS as f64,
        },
        DodgeAction::None,
    ));
    
    // 加载玩家头像图片（使用小写路径，确保兼容性）
    let p1_head_image: Handle<Image> = asset_server.load("Statics/js.jpg");
    let p2_head_image: Handle<Image> = asset_server.load("Statics/wmh.jpg");
    
    // 调试输出已禁用: println!("正在加载图片资源: Player1 -> Statics/js.jpg, Player2 -> Statics/wmh.jpg");
    // 调试输出已禁用: println!("图片句柄: P1={:?}, P2={:?}", p1_head_image.id(), p2_head_image.id());
    
    // 确定当前玩家信息（网络模式下使用）
    let current_player_id = if is_network_mode {
        if room_info.is_host {
            PlayerId::Player1
        } else {
            PlayerId::Player2
        }
    } else {
        PlayerId::Player1 // 本地模式下使用Player1作为默认值
    };
    let current_player_is_attacker = if is_network_mode {
        let is_attacker = matches!(current_player_id, PlayerId::Player1) && matches!(p1_role, PlayerRole::Attacker) ||
                          matches!(current_player_id, PlayerId::Player2) && matches!(p2_role, PlayerRole::Attacker);
        println!("[调试] current_player_id: {:?}, p1_role: {:?}, p2_role: {:?}, current_player_is_attacker: {}", 
                 current_player_id, p1_role, p2_role, is_attacker);
        is_attacker
    } else {
        false // 本地模式下不使用
    };
    
    // 为玩家1创建人形sprite（蓝色，使用js.jpg作为头部）
    spawn_humanoid_sprite(
        &mut commands, 
        PlayerId::Player1, 
        Color::rgb(0.2, 0.4, 1.0), 
        p1_pos, 
        Some(p1_head_image.clone()),
        is_network_mode,
        current_player_id == PlayerId::Player1,
        current_player_is_attacker,
    );

    // 创建玩家2（绿色）
    let p2_pos = if matches!(p2_role, PlayerRole::Attacker) { ATTACKER_START_POS } else { DEFENDER_START_POS };
    commands.spawn((
        PlayerId::Player2,
        p2_role,
        Health(PLAYER_HP),
        Transform::from_translation(p2_pos),
        Visibility::Visible,
        Collider { size: PLAYER_SIZE },
        ActionCooldown {
            last_action_time: 0.0,
            cooldown_duration: DODGE_COOLDOWN_SECONDS as f64,
        },
        DodgeAction::None,
    ));
    
    // 为玩家2创建人形sprite（绿色，使用wmh.jpg作为头部）
    spawn_humanoid_sprite(
        &mut commands, 
        PlayerId::Player2, 
        Color::rgb(0.2, 1.0, 0.4), 
        p2_pos, 
        Some(p2_head_image.clone()),
        is_network_mode,
        current_player_id == PlayerId::Player2,
        current_player_is_attacker,
    );
    
    // 调试输出已禁用: println!("[调试] 游戏实体创建完成: P1={:?} at {:?}, P2={:?} at {:?}", p1_role, p1_pos, p2_role, p2_pos);
    
    // 创建加大加宽墙体（30列×12行）
    // 调试输出已禁用: println!("[调试] 开始创建墙体...");
    let brick_cols = BRICK_COLS;
    let brick_rows = BRICK_ROWS;
    let brick_width = BRICK_WIDTH;
    let brick_height = BRICK_HEIGHT;
    
    let brick_colors = vec![Color::rgb(0.8, 0.6, 0.5), Color::rgb(0.5, 0.35, 0.25)];
    let wall_entity = commands.spawn((Wall { damaged: false, damage_positions: Vec2::ZERO },)).id();
    // 调试输出已禁用: println!("[调试] 墙体实体创建完成，开始创建砖块...");
    
    // 确定要创建的视角
    // 网络模式下，只创建当前玩家视角的墙体副本
    // 本地模式下，创建两个视角的墙体副本
    let mut view_layers_to_create: Vec<ViewLayer> = if is_network_mode {
        // 网络模式：只创建当前玩家视角的实体
        if current_player_is_attacker {
            vec![ViewLayer::AttackerView] // 进攻方视角：只创建layer 0的墙体
        } else {
            vec![ViewLayer::DefenderView] // 防守方视角：只创建layer 1的墙体
        }
    } else {
        // 本地模式：创建两个视角的副本
        vec![ViewLayer::AttackerView, ViewLayer::DefenderView]
    };
    
    println!("[调试] 将创建以下视角的墙体: {:?} (网络模式: {}, current_player_is_attacker: {})", 
             view_layers_to_create, is_network_mode, current_player_is_attacker);
    // 调试输出已禁用: println!("[调试] view_layers_to_create.len() = {}", view_layers_to_create.len());
    println!("[调试] 当前玩家ID: {:?}, 是房主: {}", 
             if is_network_mode { if room_info.is_host { "Player1" } else { "Player2" } } else { "本地模式" },
             room_info.is_host);
    
    // 强制修复：确保在网络模式下，只创建当前玩家视角的墙体
    if view_layers_to_create.len() != 1 && is_network_mode {
        // 调试输出已禁用: println!("[错误] 网络模式下，view_layers_to_create应该只包含1个元素，但实际包含 {} 个元素！", view_layers_to_create.len());
        // 调试输出已禁用: println!("[错误] 这可能导致多渲染墙的画面！强制修复：只保留第一个元素");
        // 强制修复：只保留第一个元素
        if !view_layers_to_create.is_empty() {
            let first_layer = view_layers_to_create[0];
            view_layers_to_create = vec![first_layer];
            // 调试输出已禁用: println!("[修复] 已强制修复为: {:?}", view_layers_to_create);
        }
    }
    
    // 添加调试：统计将创建的墙体数量
    let total_segments = brick_cols * brick_rows * view_layers_to_create.len();
    println!("[调试] 将创建 {} 个墙体段 ({} 列 × {} 行 × {} 个视角)", 
             total_segments, brick_cols, brick_rows, view_layers_to_create.len());
    
    for row in 0..brick_rows {
        for col in 0..brick_cols {
            let x_offset = (col as f32 - (brick_cols as f32 - 1.0) / 2.0) * brick_width;
            let y_offset = (row as f32 - (brick_rows as f32 - 1.0) / 2.0) * brick_height;
            let row_offset = if row % 2 == 1 { brick_width / 2.0 } else { 0.0 };
            let final_x_offset = x_offset + row_offset;
            let brick_color = brick_colors[(col + row) % brick_colors.len()];
            
            // 只创建当前视角的墙
            for view_layer in view_layers_to_create.iter() {
                let render_layer = match view_layer {
                    ViewLayer::AttackerView => RenderLayers::layer(0), // 进攻方视角渲染层
                    ViewLayer::DefenderView => RenderLayers::layer(1), // 防守方视角渲染层
                };
                
                // 根据视角设置不同的Z轴
                let wall_z_pos = match view_layer {
                    ViewLayer::AttackerView => 2.0, // 进攻方视角：墙在Z轴2.0（后渲染，会遮挡人物）
                    ViewLayer::DefenderView => 1.0, // 防守方视角：墙在Z轴1.0（先渲染，人物在墙之后）
                };
                
                if matches!(view_layer, ViewLayer::AttackerView) {
                    // 攻击方视角使用黑色背景填充砖块缝隙，防守方视角不需要额外背景
                let background_z = wall_z_pos - 0.1;
            commands.spawn((
                    WallBackground {
                        position: Vec2::new(final_x_offset, y_offset),
                            view_layer: *view_layer,
                    },
                SpriteBundle {
                    sprite: Sprite {
                            color: Color::BLACK,
                                custom_size: Some(Vec2::new(brick_width, brick_height)),
                        ..default()
                    },
                        transform: Transform::from_translation(Vec3::new(
                                final_x_offset,
                                WALL_POSITION.y + y_offset,
                                background_z,
                        )),
                    ..default()
                },
                    render_layer,
                ));
                }
                
                // 再创建砖块（稍小，露出黑色缝隙）
                // 检查是否需要恢复破碎状态
                let segment_pos = Vec2::new(final_x_offset, y_offset);
                let is_broken = if let Some(broken_data) = broken_wall_data.as_ref() {
                    let is_host = room_info.is_host;
                    if is_host {
                        broken_data.host_broken_segments.contains(&segment_pos)
                    } else {
                        broken_data.client_broken_segments.contains(&segment_pos)
                    }
                } else {
                    false
                };
                
                let (segment_damaged, segment_color, segment_visibility) = if is_broken {
                    // 恢复破碎状态
                    // 调试输出已禁用: println!("[恢复破碎墙体] 位置: {:?}, 视角: {:?}", segment_pos, view_layer);
                    match view_layer {
                        ViewLayer::AttackerView => (true, Color::rgba(0.0, 0.0, 0.0, 0.0), Visibility::Hidden),
                        ViewLayer::DefenderView => (true, Color::rgba(0.0, 0.0, 0.0, 0.8), Visibility::Visible),
                    }
                } else {
                    (false, brick_color, Visibility::Visible)
                };
                
                let segment_entity = commands.spawn((
                    WallSegment {
                        wall_entity,
                        position: segment_pos,
                        damaged: segment_damaged,
                        view_layer: *view_layer,
                    },
                    SpriteBundle {
                        sprite: Sprite {
                            color: segment_color,
                            custom_size: Some(Vec2::new(brick_width - 2.0, brick_height - 2.0)),
                            ..default()
                        },
                        transform: Transform::from_translation(Vec3::new(
                            final_x_offset, WALL_POSITION.y + y_offset, wall_z_pos
                        )),
                        visibility: segment_visibility,
                        ..default()
                    },
                    Collider { size: Vec2::new(brick_width - 2.0, brick_height - 2.0) }, // 碰撞框与视觉一致
                    render_layer,
                )).id();
                
                // 如果是防守方视角的破碎墙体，创建破损效果
                if is_broken && matches!(view_layer, ViewLayer::DefenderView) {
                    commands.spawn((
                        SpriteBundle {
                            sprite: Sprite { color: Color::rgba(0.0, 0.0, 0.0, 0.9), custom_size: Some(Vec2::new(30.0, 30.0)), ..default() },
                            transform: Transform::from_translation(Vec3::new(
                                final_x_offset, WALL_POSITION.y + y_offset, wall_z_pos + 0.5
                            )),
                            ..default()
                        },
                        render_layer,
                    ));
                }
            }
        }
    }
    
    // 调试：验证创建的墙体数量
    // 调试输出已禁用: println!("[调试] 墙体创建完成，共创建 {} 个墙体段", brick_cols * brick_rows * view_layers_to_create.len());

    // 创建十字准星（只在进攻方视角显示，固定在屏幕中心）
    // 网络模式下，只有当前玩家是进攻方时才创建
    if !is_network_mode || current_player_is_attacker {
        commands.spawn((
            Crosshair,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(2.0, 30.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                    ..default()
                },
            RenderLayers::layer(0), // 只在进攻方视角显示
        ));
        commands.spawn((
            Crosshair,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(30.0, 2.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                ..default()
            },
            RenderLayers::layer(0), // 只在进攻方视角显示
        ));
        commands.spawn((
            Crosshair,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(4.0, 4.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 303.0)),
                    ..default()
                },
            RenderLayers::layer(0), // 只在进攻方视角显示
        ));
    }
    
    // 创建激光指示器（只在防守方视角显示）
    // 网络模式下，只有当前玩家是防守方时才创建
    if !is_network_mode || !current_player_is_attacker {
        commands.spawn((
            LaserIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgba(1.0, 0.0, 0.0, 0.8), custom_size: Some(Vec2::new(100.0, 3.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 50.0)),
            ..default()
            },
            RenderLayers::layer(1), // 只在防守方视角显示
        ));
    }
    
    // 创建防守方视角的准星指示器（显示进攻方瞄准的位置）
    // 网络模式下，只有当前玩家是防守方时才创建
    if !is_network_mode || !current_player_is_attacker {
        commands.spawn((
            DefenderCrosshairIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(2.0, 30.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                ..default()
            },
            RenderLayers::layer(1), // 只在防守方视角显示
        ));
        commands.spawn((
            DefenderCrosshairIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(30.0, 2.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
            ..default()
            },
            RenderLayers::layer(1), // 只在防守方视角显示
        ));
        commands.spawn((
            DefenderCrosshairIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(4.0, 4.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 303.0)),
                ..default()
            },
            RenderLayers::layer(1), // 只在防守方视角显示
        ));
    }
    
    // 创建UI（根据游戏模式决定显示方式）
    setup_ui(&mut commands, font, Some(&*room_info));
    // 调试输出已禁用: println!("=== 游戏设置完成 ===");
    // 调试输出已禁用: println!("当前进攻方: {:?}", current_attacker);
    // 调试输出已禁用: println!("网络模式: {}", is_network_mode);
    // 调试输出已禁用: println!("本地模式: {}", !is_network_mode);
    if is_network_mode {
        println!("房主: {}, 当前玩家角色: {}", 
                 room_info.is_host,
                 if room_info.is_host { "进攻方" } else { "防守方" });
        // 调试输出已禁用: println!("[网络模式] 提示：每个玩家只能看到和操控自己的角色");
            } else {
        // 调试输出已禁用: println!("[本地模式] 提示：两个玩家可以同时操控各自的角色");
        // 调试输出已禁用: println!("[本地模式] 左侧窗口：进攻方视角（WASD瞄准，空格射击）");
        // 调试输出已禁用: println!("[本地模式] 右侧窗口：防守方视角（方向键移动，Ctrl下蹲，Shift侧躲）");
    }
}

/// 清理进攻方视角中属于 layer 1 的实体
/// 确保进攻方视角只渲染 layer 0 的实体
fn cleanup_attacker_view_entities_system(
    mut commands: Commands,
    room_info: Option<Res<RoomInfo>>,
    view_config: Res<ViewConfig>,
    humanoid_query: Query<(Entity, &gameplay::HumanoidPart, Option<&RenderLayers>)>,
    wall_segment_query: Query<(Entity, &gameplay::WallSegment, Option<&RenderLayers>)>,
    wall_background_query: Query<(Entity, &gameplay::WallBackground, Option<&RenderLayers>)>,
    laser_indicator_query: Query<(Entity, &gameplay::LaserIndicator, Option<&RenderLayers>)>,
    defender_crosshair_query: Query<(Entity, &DefenderCrosshairIndicator, Option<&RenderLayers>)>,
    // 查询所有带有 Sprite 和 RenderLayers 的实体（用于清理破损墙体效果等没有标记组件的实体）
    sprite_with_layers_query: Query<(Entity, &Sprite, Option<&RenderLayers>), (With<Sprite>, Without<gameplay::HumanoidPart>, Without<gameplay::WallSegment>, Without<gameplay::WallBackground>, Without<gameplay::LaserIndicator>, Without<DefenderCrosshairIndicator>, Without<Crosshair>)>,
) {
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return; // 只在网络模式下执行
    }
    
    let is_attacker_view = view_config.is_attacker_view;
    if !is_attacker_view {
        return; // 只在进攻方视角时执行
    }
    
    // 检查并删除属于 layer 1 的实体（进攻方视角不应该看到这些）
    // 先统计所有实体数量，用于调试
    let total_wall_segments: usize = wall_segment_query.iter().count();
    let total_wall_backgrounds: usize = wall_background_query.iter().count();
    let total_humanoids: usize = humanoid_query.iter().count();
    
    if total_wall_segments > 0 || total_wall_backgrounds > 0 {
        // 调试输出已禁用: println!("[清理实体] 进攻方视角：检测到 {} 个 WallSegment, {} 个 WallBackground, {} 个 HumanoidPart", 
        //              total_wall_segments, total_wall_backgrounds, total_humanoids);
        
        // 详细列出所有墙体的信息
        // 调试输出已禁用: println!("[清理实体] WallSegment 详细信息：");
        for (entity, wall_segment, render_layers) in wall_segment_query.iter() {
            let is_defender_view = matches!(wall_segment.view_layer, ViewLayer::DefenderView);
            let is_layer_0 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(0))).unwrap_or(true);
            let is_layer_1 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(1))).unwrap_or(false);
            // 调试输出已禁用: println!("  - 实体 {:?}: view_layer={:?}, render_layers={:?}, is_layer_0={}, is_layer_1={}", 
            //                  entity, wall_segment.view_layer, render_layers, is_layer_0, is_layer_1);
        }
        // 调试输出已禁用: println!("[清理实体] WallBackground 详细信息：");
        for (entity, wall_background, render_layers) in wall_background_query.iter() {
            let is_defender_view = matches!(wall_background.view_layer, ViewLayer::DefenderView);
            let is_layer_0 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(0))).unwrap_or(true);
            let is_layer_1 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(1))).unwrap_or(false);
            // 调试输出已禁用: println!("  - 实体 {:?}: view_layer={:?}, render_layers={:?}, is_layer_0={}, is_layer_1={}", 
            //                  entity, wall_background.view_layer, render_layers, is_layer_0, is_layer_1);
        }
    }
    
    let mut deleted_count = 0;
    
    // 检查 HumanoidPart 实体
    for (entity, humanoid_part, render_layers) in humanoid_query.iter() {
        if matches!(humanoid_part.view_layer, ViewLayer::DefenderView) {
            // 这是防守方视角的实体，应该删除
            // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中的防守方 HumanoidPart 实体 {:?} (view_layer: {:?})", entity, humanoid_part.view_layer);
            commands.entity(entity).despawn_recursive();
            deleted_count += 1;
        } else if let Some(rl) = render_layers {
            // 检查是否属于 layer 1
            if rl.intersects(&RenderLayers::layer(1)) {
                // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中属于 layer 1 的 HumanoidPart 实体 {:?}", entity);
                commands.entity(entity).despawn_recursive();
                deleted_count += 1;
            }
        }
    }
    
    // 检查 WallSegment 实体 - 最激进策略：只保留 view_layer 为 AttackerView 且属于 layer 0 的墙体
    // 删除所有其他墙体，包括没有 RenderLayers 的墙体（如果 view_layer 是 DefenderView）
    let mut wall_segments_to_keep = Vec::new();
    for (entity, wall_segment, render_layers) in wall_segment_query.iter() {
        let is_defender_view = matches!(wall_segment.view_layer, ViewLayer::DefenderView);
        let is_layer_0 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(0))).unwrap_or(true); // 默认属于 layer 0
        let is_layer_1 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(1))).unwrap_or(false);
        
        // 只保留 view_layer 为 AttackerView 且属于 layer 0 的墙体
        let should_keep = !is_defender_view && is_layer_0 && !is_layer_1;
        
        if should_keep {
            // 额外验证：确保 RenderLayers 正确
            if let Some(rl) = render_layers {
                let expected_layer = RenderLayers::layer(0);
                if rl.intersects(&expected_layer) && !rl.intersects(&RenderLayers::layer(1)) {
                    wall_segments_to_keep.push(entity);
                } else {
                    println!("[清理实体] 删除进攻方视角中的 WallSegment 实体 {:?} (RenderLayers 不正确: {:?})", 
                             entity, render_layers);
                    commands.entity(entity).despawn_recursive();
                    deleted_count += 1;
                }
            } else {
                // 没有 RenderLayers，默认属于 layer 0，如果 view_layer 是 AttackerView，保留
                wall_segments_to_keep.push(entity);
            }
        } else {
            println!("[清理实体] 删除进攻方视角中的 WallSegment 实体 {:?} (view_layer: {:?}, render_layers: {:?}, is_layer_0: {}, is_layer_1: {})", 
                     entity, wall_segment.view_layer, render_layers, is_layer_0, is_layer_1);
            commands.entity(entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    
    // 如果保留的墙体数量超过预期（30列×12行=360个），说明有重复，删除多余的
    let expected_wall_count = BRICK_COLS * BRICK_ROWS; // brick_cols * brick_rows
    if wall_segments_to_keep.len() > expected_wall_count {
        println!("[清理实体] 警告：检测到 {} 个 WallSegment，超过预期的 {} 个！可能有重复创建", 
                 wall_segments_to_keep.len(), expected_wall_count);
        // 删除多余的墙体（保留前 expected_wall_count 个）
        for entity in wall_segments_to_keep.iter().skip(expected_wall_count) {
            // 调试输出已禁用: println!("[清理实体] 删除重复的 WallSegment 实体 {:?}", entity);
            commands.entity(*entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    
    // 检查 WallBackground 实体 - 最激进策略：只保留 view_layer 为 AttackerView 且属于 layer 0 的墙体背景
    let mut wall_backgrounds_to_keep = Vec::new();
    for (entity, wall_background, render_layers) in wall_background_query.iter() {
        let is_defender_view = matches!(wall_background.view_layer, ViewLayer::DefenderView);
        let is_layer_0 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(0))).unwrap_or(true); // 默认属于 layer 0
        let is_layer_1 = render_layers.map(|rl| rl.intersects(&RenderLayers::layer(1))).unwrap_or(false);
        
        // 只保留 view_layer 为 AttackerView 且属于 layer 0 的墙体背景
        let should_keep = !is_defender_view && is_layer_0 && !is_layer_1;
        
        if should_keep {
            // 额外验证：确保 RenderLayers 正确
            if let Some(rl) = render_layers {
                let expected_layer = RenderLayers::layer(0);
                if rl.intersects(&expected_layer) && !rl.intersects(&RenderLayers::layer(1)) {
                    wall_backgrounds_to_keep.push(entity);
                } else {
                    println!("[清理实体] 删除进攻方视角中的 WallBackground 实体 {:?} (RenderLayers 不正确: {:?})", 
                             entity, render_layers);
                    commands.entity(entity).despawn_recursive();
                    deleted_count += 1;
                }
            } else {
                // 没有 RenderLayers，默认属于 layer 0，如果 view_layer 是 AttackerView，保留
                wall_backgrounds_to_keep.push(entity);
            }
        } else {
            println!("[清理实体] 删除进攻方视角中的 WallBackground 实体 {:?} (view_layer: {:?}, render_layers: {:?}, is_layer_0: {}, is_layer_1: {})", 
                     entity, wall_background.view_layer, render_layers, is_layer_0, is_layer_1);
            commands.entity(entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    
    // 如果保留的墙体背景数量超过预期（30列×12行=360个），说明有重复，删除多余的
    let expected_wall_count = BRICK_COLS * BRICK_ROWS; // brick_cols * brick_rows
    if wall_backgrounds_to_keep.len() > expected_wall_count {
        println!("[清理实体] 警告：检测到 {} 个 WallBackground，超过预期的 {} 个！可能有重复创建", 
                 wall_backgrounds_to_keep.len(), expected_wall_count);
        // 删除多余的墙体背景（保留前 expected_wall_count 个）
        for entity in wall_backgrounds_to_keep.iter().skip(expected_wall_count) {
            // 调试输出已禁用: println!("[清理实体] 删除重复的 WallBackground 实体 {:?}", entity);
            commands.entity(*entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    
    // 检查 LaserIndicator 实体（防守方视角专用）
    for (entity, _, render_layers) in laser_indicator_query.iter() {
        if let Some(rl) = render_layers {
            if rl.intersects(&RenderLayers::layer(1)) {
                // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中的 LaserIndicator 实体 {:?} (属于 layer 1)", entity);
                commands.entity(entity).despawn_recursive();
                deleted_count += 1;
            }
        } else {
            // 没有 RenderLayers，默认属于 layer 0，但 LaserIndicator 应该在 layer 1
            // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中的 LaserIndicator 实体 {:?} (无 RenderLayers，但应该是防守方专用)", entity);
            commands.entity(entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    
    // 检查 DefenderCrosshairIndicator 实体（防守方视角专用）
    for (entity, _, render_layers) in defender_crosshair_query.iter() {
        if let Some(rl) = render_layers {
            if rl.intersects(&RenderLayers::layer(1)) {
                // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中的 DefenderCrosshairIndicator 实体 {:?} (属于 layer 1)", entity);
                commands.entity(entity).despawn_recursive();
                deleted_count += 1;
            }
        } else {
            // 没有 RenderLayers，默认属于 layer 0，但 DefenderCrosshairIndicator 应该在 layer 1
            // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中的 DefenderCrosshairIndicator 实体 {:?} (无 RenderLayers，但应该是防守方专用)", entity);
            commands.entity(entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    
    // 检查所有其他带有 Sprite 和 RenderLayers 的实体（用于清理破损墙体效果等）
    // 最激进策略：删除所有不属于 layer 0 的 Sprite 实体，或者没有 RenderLayers 的 Sprite 实体
    // 调试输出已禁用: println!("[清理实体] 开始检查其他 Sprite 实体...");
    let mut sprite_entities_to_delete = Vec::new();
    for (entity, _sprite, render_layers) in sprite_with_layers_query.iter() {
        let should_delete = if let Some(rl) = render_layers {
            let is_layer_0 = rl.intersects(&RenderLayers::layer(0));
            let is_layer_1 = rl.intersects(&RenderLayers::layer(1));
            // 如果属于 layer 1，或者不属于 layer 0，都删除
            is_layer_1 || !is_layer_0
        } else {
            // 没有 RenderLayers，默认属于 layer 0，但为了安全，也检查一下
            // 如果用户说还是有问题，我们可以更激进地删除所有没有 RenderLayers 的 Sprite
            false // 暂时保留，如果还有问题再删除
        };
        
        if should_delete {
            sprite_entities_to_delete.push((entity, render_layers));
        }
    }
    
    // 逐个删除 Sprite 实体
    for (entity, render_layers) in sprite_entities_to_delete {
        // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中的 Sprite 实体 {:?} (render_layers: {:?})", entity, render_layers);
        commands.entity(entity).despawn_recursive();
        deleted_count += 1;
    }
    
    // 如果用户说还是有问题，我们可以更激进地删除所有没有 RenderLayers 的 Sprite
    // 暂时注释掉，如果还有问题再启用
    /*
    for (entity, _sprite, render_layers) in sprite_with_layers_query.iter() {
        if render_layers.is_none() {
            // 调试输出已禁用: println!("[清理实体] 删除进攻方视角中没有 RenderLayers 的 Sprite 实体 {:?}", entity);
            commands.entity(entity).despawn_recursive();
            deleted_count += 1;
        }
    }
    */
    
    // 额外清理：删除所有没有 RenderLayers 但可能属于 layer 1 的 Sprite 实体
    // 这些可能是破损墙体效果等没有标记组件的实体
    // 调试输出已禁用: println!("[清理实体] 开始检查没有 RenderLayers 的 Sprite 实体...");
    let mut sprites_without_layers = Vec::new();
    for (entity, _sprite, render_layers) in sprite_with_layers_query.iter() {
        if render_layers.is_none() {
            // 没有 RenderLayers，默认属于 layer 0
            // 但为了安全，我们可以检查一下 Transform 的 Z 值
            // 如果 Z 值接近防守方视角的 Z 值（1.0-2.0），可能是防守方的实体
            sprites_without_layers.push(entity);
        }
    }
    
    // 暂时不删除没有 RenderLayers 的 Sprite，因为默认属于 layer 0
    // 如果用户说还是有问题，我们可以更激进地删除
    
    if deleted_count > 0 {
        // 调试输出已禁用: println!("[清理实体] ========== 清理完成 ==========");
        // 调试输出已禁用: println!("[清理实体] 共删除 {} 个不应该在进攻方视角显示的实体", deleted_count);
        // 调试输出已禁用: println!("[清理实体] 请检查画面，如果还有问题，告诉我具体是什么元素多显示了");
        // 调试输出已禁用: println!("[清理实体] 如果画面正常，说明问题已解决");
    } else {
        // 调试输出已禁用: println!("[清理实体] 没有检测到需要删除的实体");
        // 调试输出已禁用: println!("[清理实体] 如果画面还是有问题，可能是其他原因（如相机、渲染层等）");
        // 调试输出已禁用: println!("[清理实体] 或者可能是没有 RenderLayers 的 Sprite 实体导致的");
    }
}

/// 清理相机上多余的组件
/// 直接移除相机上不应该存在的组件，防止显示问题
fn cleanup_camera_components_system(
    mut commands: Commands,
    room_info: Option<Res<RoomInfo>>,
    view_config: Res<ViewConfig>,
    all_cameras: Query<Entity, With<Camera2d>>,
    camera_with_player_id: Query<Entity, (With<Camera2d>, With<PlayerId>)>,
    camera_with_player_camera: Query<Entity, (With<Camera2d>, With<gameplay::PlayerCamera>)>,
    mut camera_queries: ParamSet<(
        Query<(Entity, &Camera, Option<&DefenderCamera>, Option<&IsDefaultUiCamera>, Option<&RenderLayers>), (With<Camera2d>, Without<gameplay::PlayerCamera>)>,
    )>,
) {
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return; // 只在网络模式下执行
    }
    
    let is_attacker_view = view_config.is_attacker_view;
    
    // 移除所有相机上的 PlayerId 组件
    for entity in camera_with_player_id.iter() {
        // 调试输出已禁用: println!("[清理相机组件] 移除相机 {:?} 的 PlayerId 组件", entity);
        commands.entity(entity).remove::<PlayerId>();
    }
    
    // 移除所有相机上的 PlayerCamera 组件（网络模式不应该有）
    for entity in camera_with_player_camera.iter() {
        // 调试输出已禁用: println!("[清理相机组件] 移除相机 {:?} 的 PlayerCamera 组件", entity);
        commands.entity(entity).remove::<gameplay::PlayerCamera>();
    }
    
    // 确保游戏相机的 RenderLayers 设置正确
    let camera_query = camera_queries.p0();
    for (entity, camera, defender_camera, is_ui_camera, render_layers) in camera_query.iter() {
        if is_ui_camera.is_some() {
            continue; // UI相机跳过
        }
        
        if camera.order == 0 {
            // 游戏相机：确保 RenderLayers 设置正确
            let should_be_layer = if defender_camera.is_some() {
                1 // 防守方相机：layer 1
            } else {
                0 // 进攻方相机：layer 0
            };
            
            // 检查当前的 RenderLayers 是否正确
            let current_layer_ok = render_layers.map(|rl| {
                let expected_layer = RenderLayers::layer(should_be_layer);
                rl.intersects(&expected_layer)
            }).unwrap_or(false);
            
            if !current_layer_ok {
                println!("[清理相机组件] 修复相机 {:?} 的 RenderLayers (当前: {:?}, 应该: layer({}))", 
                         entity, render_layers, should_be_layer);
                commands.entity(entity).insert(RenderLayers::layer(should_be_layer));
            }
        }
    }
}

/// 调试系统：打印所有相机的详细信息
/// 只在检测到多个相机或相机状态异常时输出
fn debug_all_cameras_system(
    room_info: Option<Res<RoomInfo>>,
    view_config: Res<ViewConfig>,
    camera_query: Query<(Entity, &Camera, Option<&DefenderCamera>, Option<&IsDefaultUiCamera>, Option<&RenderLayers>), With<Camera2d>>,
) {
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    let is_attacker_view = view_config.is_attacker_view;
    let mut game_cameras = Vec::new();
    let mut ui_cameras = Vec::new();
    
    for (entity, camera, defender_camera, is_ui_camera, render_layers) in camera_query.iter() {
        if is_ui_camera.is_some() {
            ui_cameras.push((entity, camera.is_active, camera.order, render_layers));
        } else if camera.order == 0 {
            game_cameras.push((entity, camera.is_active, defender_camera.is_some(), render_layers));
        }
    }
    
    // 只在检测到多个相机或相机状态异常时输出
    let should_print = game_cameras.len() > 1 || ui_cameras.len() > 1 || 
                       game_cameras.iter().filter(|(_, is_active, _, _)| *is_active).count() > 1;
    
    if should_print {
        // 调试输出已禁用: println!("[相机调试] ========== 当前所有相机信息 ==========");
        // 调试输出已禁用: println!("[相机调试] 当前视图: {}", if is_attacker_view { "进攻方" } else { "防守方" });
        println!("[相机调试] 游戏相机数量: {} (激活: {})", 
                 game_cameras.len(), 
                 game_cameras.iter().filter(|(_, is_active, _, _)| *is_active).count());
        for (entity, is_active, is_defender, render_layers) in game_cameras.iter() {
            println!("[相机调试]   游戏相机 {:?}: is_active={}, is_defender={}, render_layers={:?}", 
                     entity, is_active, is_defender, render_layers);
        }
        println!("[相机调试] UI相机数量: {} (激活: {})", 
                 ui_cameras.len(),
                 ui_cameras.iter().filter(|(_, is_active, _, _)| *is_active).count());
        for (entity, is_active, order, render_layers) in ui_cameras.iter() {
            println!("[相机调试]   UI相机 {:?}: is_active={}, order={}, render_layers={:?}", 
                     entity, is_active, order, render_layers);
        }
        // 调试输出已禁用: println!("[相机调试] ========================================");
    }
}

/// 确保网络模式下只有一个游戏相机和一个UI相机处于激活状态
/// 这个系统用于防止多个相机同时渲染导致显示两个画面
/// 优化：使用缓存机制，只在相机状态改变时检查
/// 更新角色切换缓冲计时器
fn update_role_switch_cooldown(
    mut cooldown: ResMut<RoleSwitchCooldown>,
    time: Res<Time>,
) {
    cooldown.timer.tick(time.delta());
}

/// 紧急检查：确保UI相机存在
/// 如果没有任何UI相机（IsDefaultUiCamera），立即创建一个
fn ensure_ui_camera_exists_system(
    mut commands: Commands,
    room_info: Option<Res<RoomInfo>>,
    view_config: Res<ViewConfig>,
    camera_query: Query<(Entity, &Camera, Option<&IsDefaultUiCamera>), With<Camera2d>>,
    mut ui_camera_entities: Option<ResMut<UiCameraEntities>>,
) {
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return; // 只在网络模式下执行
    }
    
    // 检查是否有激活的UI相机
    let mut has_active_ui_camera = false;
    for (entity, camera, is_ui_camera) in camera_query.iter() {
        if is_ui_camera.is_some() && camera.is_active {
            has_active_ui_camera = true;
            break;
        }
    }
    
    // 如果没有激活的UI相机，立即创建一个
    if !has_active_ui_camera {
        // 尝试使用游戏相机作为UI相机（如果是进攻方视图）
        if view_config.is_attacker_view {
            // 查找游戏相机（order=0，没有IsDefaultUiCamera）
            for (entity, camera, is_ui_camera) in camera_query.iter() {
                if camera.order == 0 && is_ui_camera.is_none() && camera.is_active {
                    // 给游戏相机添加IsDefaultUiCamera
                    commands.entity(entity).insert(IsDefaultUiCamera);
                    commands.entity(entity).insert(Camera {
                        is_active: true,
                        ..default()
                    });
                    println!("[紧急修复] 进攻方视图：给游戏相机 {:?} 添加IsDefaultUiCamera", entity);
                    if let Some(mut ui_cameras) = ui_camera_entities.as_mut() {
                        ui_cameras.attacker_ui_camera = entity;
                    }
                    return;
                }
            }
        }
        
        // 创建防守方UI相机
        let defender_ui_camera = commands.spawn((
            Camera2dBundle {
                camera: Camera {
                    order: 1000,
                    clear_color: ClearColorConfig::None,
                    is_active: true,
                    ..default()
                },
                ..default()
            },
            IsDefaultUiCamera,
        )).id();
        println!("[紧急修复] 创建防守方UI相机: {:?}", defender_ui_camera);
        if let Some(mut ui_cameras) = ui_camera_entities.as_mut() {
            ui_cameras.defender_ui_camera = defender_ui_camera;
        }
    }
}

fn ensure_single_active_camera_system(
    mut commands: Commands,
    room_info: Option<Res<RoomInfo>>,
    view_config: Res<ViewConfig>,
    mut camera_state_cache: ResMut<CameraStateCache>,
    cooldown: Res<RoleSwitchCooldown>,
    mut camera_queries: ParamSet<(
        Query<Entity, (With<Camera2d>, Or<(Added<Camera>, Added<DefenderCamera>, Added<IsDefaultUiCamera>)>)>,
        Query<(Entity, &Camera, &Projection, Option<&DefenderCamera>, Option<&IsDefaultUiCamera>), (With<Camera2d>, Without<gameplay::PlayerCamera>)>,
        Query<(Entity, &mut Camera, &Projection, Option<&DefenderCamera>, Option<&IsDefaultUiCamera>), (With<Camera2d>, Without<gameplay::PlayerCamera>)>,
    )>,
) {
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return; // 只在网络模式下执行
    }
    
    // 如果在角色切换缓冲期间，不执行删除操作（只执行激活/停用操作）
    let is_in_cooldown = !cooldown.timer.finished();
    
    // 检查是否有新相机添加（使用 ParamSet 的第一个查询）
    let has_camera_changes = !camera_queries.p0().is_empty();
    
    // 如果没有相机改变，且缓存状态与当前视图配置一致，则快速返回
    if !has_camera_changes && !camera_state_cache.needs_check {
        if camera_state_cache.last_is_attacker_view == view_config.is_attacker_view {
            // 快速检查：如果只有一个游戏相机和一个UI相机，且状态正确，则直接返回
            if camera_state_cache.last_game_camera_count == 1 && camera_state_cache.last_ui_camera_count <= 1 {
                return; // 状态没有改变，跳过检查
            }
        }
    }
    
    // 重置检查标记
    camera_state_cache.needs_check = false;
    
    // 快速检查：如果只有一个游戏相机和一个UI相机，且状态正确，则快速返回（避免每帧都检查）
    let mut game_camera_count = 0;
    let mut ui_camera_count = 0;
    let mut game_camera_entity = None;
    let mut game_camera_active = false;
    let mut game_camera_scale = 1.0;
    let mut game_camera_is_defender = false;
    
    // 使用只读查询收集相机信息
    {
        let read_query = camera_queries.p1();
        for (entity, camera, projection, defender_camera, is_ui_camera) in read_query.iter() {
            if is_ui_camera.is_some() {
                ui_camera_count += 1;
            } else if camera.order == 0 {
                game_camera_count += 1;
                game_camera_entity = Some(entity);
                game_camera_active = camera.is_active;
                game_camera_is_defender = defender_camera.is_some();
                if let Projection::Orthographic(ortho) = projection {
                    game_camera_scale = ortho.scale;
                }
            }
        }
    }
    
    // 更新缓存
    camera_state_cache.last_game_camera_count = game_camera_count;
    camera_state_cache.last_ui_camera_count = ui_camera_count;
    camera_state_cache.last_is_attacker_view = view_config.is_attacker_view;
    
    // 如果只有一个游戏相机和一个UI相机，且状态正确，则快速返回
    let is_attacker_view = view_config.is_attacker_view;
    if game_camera_count == 1 && ui_camera_count <= 1 {
        let should_be_active = if is_attacker_view {
            !game_camera_is_defender && (game_camera_scale - 0.5).abs() < 0.1
        } else {
            game_camera_is_defender && (game_camera_scale - 1.5).abs() < 0.1
        };
        
        if should_be_active {
            // 快速修复：确保游戏相机是激活的
        if let Some(entity) = game_camera_entity {
            let mut write_query = camera_queries.p2();
                if let Ok((_, mut camera, _, _, _)) = write_query.get_mut(entity) {
                    if !camera.is_active {
                        camera.is_active = true;
                        if is_attacker_view {
                            camera.clear_color = ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0));
                        }
                    }
                }
            }
            return; // 只有一个游戏相机，且状态正确，直接返回
        }
    }
    
    // 如果有多个相机或状态不正确，执行完整的清理逻辑
    let is_attacker_view = view_config.is_attacker_view;
    let mut game_cameras = Vec::new();
    let mut ui_cameras = Vec::new();
    
    {
        let mut write_query = camera_queries.p2();
        for (entity, mut camera, projection, defender_camera, is_ui_camera) in write_query.iter_mut() {
        if is_ui_camera.is_some() {
            // UI相机（有IsDefaultUiCamera组件）
            ui_cameras.push((entity, camera.is_active, camera.order));
        } else if camera.order == 0 {
            // 游戏相机（order = 0，没有IsDefaultUiCamera组件）
            // 检查 scale：进攻方相机 scale=0.5，防守方相机 scale=1.5
            let camera_scale = if let Projection::Orthographic(ortho) = projection {
                ortho.scale
            } else {
                1.0
            };
            game_cameras.push((entity, camera.is_active, defender_camera.is_some(), camera.order, camera_scale));
        }
        // 忽略其他相机（order > 0 且没有IsDefaultUiCamera的相机不应该存在）
        }
    }
    
    // 如果检测到多个游戏相机，直接删除多余的相机（只保留应该激活的那个）
    if game_cameras.len() > 1 {
        // 调试输出已禁用: println!("[严重错误] 检测到 {} 个游戏相机！这会导致显示两个画面！", game_cameras.len());
        // 调试输出已禁用: println!("[严重错误] 详细列表：");
        for (entity, is_active, is_defender, _, scale) in game_cameras.iter() {
            // 调试输出已禁用: println!("  - 相机 {:?}: is_active={}, is_defender={}, scale={}", entity, is_active, is_defender, scale);
        }
        // 调试输出已禁用: println!("[严重错误] 将删除多余的相机，只保留应该激活的相机");
        
        let mut should_be_active_entity = None;
        for (entity, _, is_defender_camera, _, scale) in game_cameras.iter() {
            let should_be_active = if is_attacker_view {
                // 进攻方视图：保留 scale=0.5 的相机（非防守方相机）
                !*is_defender_camera && (*scale - 0.5).abs() < 0.1
            } else {
                // 防守方视图：保留 scale=1.5 的相机（防守方相机）
                *is_defender_camera && (*scale - 1.5).abs() < 0.1
            };
            if should_be_active {
                should_be_active_entity = Some(*entity);
                println!("[修复] 确定应该保留的相机: {:?} ({}视图, scale={})", 
                         entity, if is_attacker_view { "进攻方" } else { "防守方" }, scale);
                break;
            }
        }
        
        if should_be_active_entity.is_none() {
            // 调试输出已禁用: println!("[严重错误] 无法确定应该保留的相机！将保留第一个非防守方相机（进攻方视图）或第一个防守方相机（防守方视图）");
            // 如果找不到应该激活的相机，保留第一个符合条件的相机
            for (entity, _, is_defender_camera, _, scale) in game_cameras.iter() {
                let should_be_active = if is_attacker_view {
                    (*scale - 0.5).abs() < 0.1 // 进攻方：scale 接近 0.5
                } else {
                    (*scale - 1.5).abs() < 0.1 // 防守方：scale 接近 1.5
                };
                if should_be_active {
                    should_be_active_entity = Some(*entity);
                    // 调试输出已禁用: println!("[修复] 根据 scale 选择相机: {:?} (scale={})", entity, scale);
                    break;
                }
            }
            // 如果还是找不到，保留第一个相机
            if should_be_active_entity.is_none() && !game_cameras.is_empty() {
                should_be_active_entity = Some(game_cameras[0].0);
                // 调试输出已禁用: println!("[修复] 使用第一个相机作为默认: {:?}", should_be_active_entity);
            }
        }
        
        // 删除所有多余的相机，只保留应该激活的相机
        // 对于进攻方视图，特别删除 scale=1.5 的相机（会显示缩小画面）
        for (entity, _, _, _, scale) in game_cameras.iter() {
            if Some(*entity) != should_be_active_entity {
                // 调试输出已禁用: println!("[修复] 删除多余的游戏相机 {:?} (scale={}, 这会导致多渲染一次游玩画面)", entity, scale);
                commands.entity(*entity).despawn_recursive();
            } else {
                println!("[修复] 保留游戏相机 {:?} ({}视图, scale={})，确保激活并清除背景", 
                         entity, if is_attacker_view { "进攻方" } else { "防守方" }, scale);
                // 确保这个相机是激活的，并且对于进攻方视图，使用黑色背景清除所有内容
                let mut write_query = camera_queries.p2();
                if let Ok((_, mut camera, _, _, _)) = write_query.get_mut(*entity) {
                    camera.is_active = true;
                    if is_attacker_view {
                        // 进攻方视图：使用黑色背景清除所有内容
                        camera.clear_color = ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0));
                        // 调试输出已禁用: println!("[修复] 进攻方相机 {:?} 使用黑色背景清除所有内容", entity);
                    }
                } else {
                    // 如果无法获取可变引用，使用 commands 插入
                    commands.entity(*entity).insert(Camera {
                        is_active: true,
                        clear_color: if is_attacker_view {
                            ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0))
                        } else {
                            ClearColorConfig::default()
                        },
                        ..default()
                    });
                }
            }
        }
        return; // 删除相机后，下一帧再检查
    }
    
    // 确保只有一个游戏相机处于激活状态，且是当前角色对应的相机
    
    // 如果只有一个游戏相机，确保它是激活的且是正确的相机
    // 对于进攻方视图，确保 scale=0.5，如果是 scale=1.5 则删除
    if game_cameras.len() == 1 {
        let (entity, is_active, is_defender_camera, _, scale) = game_cameras[0];
        let should_be_active = if is_attacker_view {
            // 进攻方视图：只保留 scale=0.5 的相机
            !is_defender_camera && (scale - 0.5).abs() < 0.1
        } else {
            // 防守方视图：只保留 scale=1.5 的相机
            is_defender_camera && (scale - 1.5).abs() < 0.1
        };
        
        if !should_be_active {
            // 如果相机不符合要求，在缓冲期间只停用，不删除
            if is_in_cooldown {
                println!("[修复] 缓冲期间：停用不符合要求的游戏相机 {:?} (scale={}, is_defender={}, 当前视图: {})", 
                         entity, scale, is_defender_camera, if is_attacker_view { "进攻方" } else { "防守方" });
                let mut write_query = camera_queries.p2();
                if let Ok((_, mut camera, _, _, _)) = write_query.get_mut(entity) {
                    camera.is_active = false;
                } else {
                    commands.entity(entity).insert(Camera {
                        is_active: false,
                        ..default()
                    });
                }
                return;
            } else {
                // 缓冲期过后，删除不符合要求的相机
                println!("[修复] 删除不符合要求的游戏相机 {:?} (scale={}, is_defender={}, 当前视图: {})", 
                         entity, scale, is_defender_camera, if is_attacker_view { "进攻方" } else { "防守方" });
                commands.entity(entity).despawn_recursive();
                return;
            }
        }
        
        // 对于进攻方视图，确保相机使用 clear_color 清除背景
        // 通过 write_query 获取相机的可变引用
        let mut write_query = camera_queries.p2();
        if let Ok((_, mut camera, _, _, _)) = write_query.get_mut(entity) {
            if is_attacker_view {
                // 确保进攻方相机使用黑色背景清除所有内容
                camera.is_active = true;
                camera.clear_color = ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0)); // 黑色背景，清除所有内容
                // 调试输出已禁用: println!("[修复] 确保进攻方相机 {:?} 使用黑色背景清除所有内容", entity);
            } else {
                if !is_active {
                    println!("[修复] 修复游戏相机 {:?} 的激活状态 (is_active={}, should_be_active={})", 
                             entity, is_active, should_be_active);
                    camera.is_active = true;
                }
            }
        }
    }
    
    // 确保只有一个UI相机处于激活状态
    if ui_cameras.len() > 1 {
        // 调试输出已禁用: println!("[警告] 检测到 {} 个UI相机，只保留第一个激活的相机", ui_cameras.len());
        let mut found_active = false;
        for (entity, is_active, order) in ui_cameras.iter() {
            if *is_active && !found_active {
                found_active = true;
                // 调试输出已禁用: println!("[修复] 保持UI相机 {:?} (order: {}) 激活", entity, order);
            } else {
                // 调试输出已禁用: println!("[修复] 停用UI相机 {:?} (order: {})", entity, order);
                commands.entity(*entity).insert(Camera {
                    is_active: false,
                    ..default()
                });
            }
        }
    }
}

/// 角色切换时重建游戏实体系统
/// 在网络模式下，当角色切换时，需要清理并重新创建游戏实体（因为只创建了当前视角的实体）
fn recreate_game_entities_on_role_switch_system(
    mut commands: Commands,
    mut recreate_flag: ResMut<RecreateGameEntitiesOnRoleSwitch>,
    mut camera_state_cache: ResMut<CameraStateCache>,
    room_info: Option<Res<RoomInfo>>,
    asset_server: Res<AssetServer>,
    broken_wall_data: Option<Res<BrokenWallData>>,
    player_query: Query<(Entity, &PlayerId, &PlayerRole, &Transform), (With<PlayerId>, Without<gameplay::HumanoidPart>)>,
    humanoid_query: Query<Entity, With<gameplay::HumanoidPart>>,
    wall_segment_query: Query<Entity, With<gameplay::WallSegment>>,
    wall_background_query: Query<Entity, With<gameplay::WallBackground>>,
    wall_query: Query<Entity, With<gameplay::Wall>>, // 也需要清理Wall实体
    crosshair_query: Query<Entity, With<Crosshair>>,
    laser_indicator_query: Query<Entity, With<gameplay::LaserIndicator>>,
    defender_crosshair_query: Query<Entity, With<DefenderCrosshairIndicator>>,
    camera_query: Query<(Entity, &Camera, Option<&IsDefaultUiCamera>), With<Camera2d>>,
    mut ui_camera_entities: Option<ResMut<UiCameraEntities>>,
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode || !recreate_flag.should_recreate {
        return;
    }
    
    // 调试输出已禁用: println!("[角色切换] ========== 开始重建游戏实体 ==========");
    
    // 重置标志
    recreate_flag.should_recreate = false;
    
    // 标记相机状态缓存需要检查（重建实体时可能会创建/删除相机）
    camera_state_cache.needs_check = true;
    
    // 确定当前玩家信息
    let is_host = room_info.as_ref().unwrap().is_host;
    let current_player_id = if is_host { PlayerId::Player1 } else { PlayerId::Player2 };
    
    // 查询当前玩家的角色
    let mut current_player_role = None;
    let mut current_player_pos = Vec3::ZERO;
    for (_, id, role, transform) in player_query.iter() {
        if *id == current_player_id {
            current_player_role = Some(*role);
            current_player_pos = transform.translation;
            break;
        }
    }
    
    if current_player_role.is_none() {
        // 调试输出已禁用: println!("[错误] 未找到当前玩家的角色，无法重建游戏实体");
        return;
    }
    
    let current_player_is_attacker = matches!(current_player_role.unwrap(), PlayerRole::Attacker);
    println!("[角色切换] 当前玩家: {:?}, 角色: {:?}, 是进攻方: {}", 
             current_player_id, current_player_role.unwrap(), current_player_is_attacker);
    
    // 处理UI相机的创建/删除（根据新角色）
    // 查询所有相机，找到游戏相机和UI相机
    let mut game_camera_entity: Option<Entity> = None;
    let mut ui_camera_entity: Option<Entity> = None;
    for (entity, camera, is_ui_camera) in camera_query.iter() {
        if camera.order == 0 {
            // 游戏相机
            game_camera_entity = Some(entity);
        } else if is_ui_camera.is_some() {
            // UI相机（order > 0 且有IsDefaultUiCamera）
            ui_camera_entity = Some(entity);
        }
    }
    
    if let Some(game_camera) = game_camera_entity {
        if current_player_is_attacker {
            // 进攻方：游戏相机作为UI相机
            // 先确保游戏相机有IsDefaultUiCamera（在删除UI相机之前）
            commands.entity(game_camera).insert(IsDefaultUiCamera);
            // 确保游戏相机是激活的
            commands.entity(game_camera).insert(Camera {
                is_active: true,
                ..default()
            });
            // 删除单独的UI相机（如果存在）
            if let Some(ui_camera) = ui_camera_entity {
                // 调试输出已禁用: println!("[角色切换] 删除防守方的UI相机: {:?}", ui_camera);
                commands.entity(ui_camera).despawn_recursive();
            }
            // 调试输出已禁用: println!("[角色切换] 进攻方：游戏相机 {:?} 作为UI相机", game_camera);
            // 更新UiCameraEntities资源
            if let Some(mut ui_cameras) = ui_camera_entities.as_mut() {
                ui_cameras.attacker_ui_camera = game_camera;
            }
        } else {
            // 防守方：使用单独的UI相机
            // 先创建防守方的UI相机（如果不存在），确保在移除游戏相机的IsDefaultUiCamera之前有UI相机
            let defender_ui_camera_entity = if ui_camera_entity.is_some() {
                ui_camera_entity.unwrap()
            } else {
                let new_ui_camera = commands.spawn((
                    Camera2dBundle {
                        camera: Camera {
                            order: 1000,
                            clear_color: ClearColorConfig::None,
                            is_active: true,
                            ..default()
                        },
                        ..default()
                    },
                    IsDefaultUiCamera,
                )).id();
                // 调试输出已禁用: println!("[角色切换] 创建防守方UI相机: {:?}", new_ui_camera);
                // 更新UiCameraEntities资源
                if let Some(mut ui_cameras) = ui_camera_entities.as_mut() {
                    ui_cameras.defender_ui_camera = new_ui_camera;
                }
                new_ui_camera
            };
            // 确保UI相机是激活的且有IsDefaultUiCamera
            commands.entity(defender_ui_camera_entity).insert(IsDefaultUiCamera);
            commands.entity(defender_ui_camera_entity).insert(Camera {
                order: 1000,
                clear_color: ClearColorConfig::None,
                is_active: true,
                ..default()
            });
            // 移除游戏相机的IsDefaultUiCamera（在确保UI相机存在之后）
            commands.entity(game_camera).remove::<IsDefaultUiCamera>();
            // 调试输出已禁用: println!("[角色切换] 防守方：移除游戏相机 {:?} 的IsDefaultUiCamera", game_camera);
        }
    } else {
        // 如果没有游戏相机，创建一个临时的UI相机（紧急情况）
        println!("[警告] 角色切换时未找到游戏相机，创建临时UI相机");
        let temp_ui_camera = commands.spawn((
            Camera2dBundle {
                camera: Camera {
                    order: 1000,
                    clear_color: ClearColorConfig::None,
                    is_active: true,
                    ..default()
                },
                ..default()
            },
            IsDefaultUiCamera,
        )).id();
        if let Some(mut ui_cameras) = ui_camera_entities.as_mut() {
            ui_cameras.defender_ui_camera = temp_ui_camera;
        }
    }
    
    // 清理旧的游戏实体（不包括玩家实体）
    let mut despawn_count = 0;
    for entity in humanoid_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    for entity in wall_segment_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    for entity in wall_background_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    for entity in wall_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    for entity in crosshair_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    for entity in laser_indicator_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    for entity in defender_crosshair_query.iter() {
        commands.entity(entity).despawn_recursive();
        despawn_count += 1;
    }
    // 调试输出已禁用: println!("[角色切换] 已清理 {} 个游戏实体", despawn_count);
    
    // 重新创建游戏实体（只创建当前玩家视角的实体）
    // 加载玩家头像图片
    let p1_head_image: Handle<Image> = asset_server.load("Statics/js.jpg");
    let p2_head_image: Handle<Image> = asset_server.load("Statics/wmh.jpg");
    
    // 查询所有玩家的位置和角色
    let mut p1_pos = Vec3::ZERO;
    let mut p1_role = PlayerRole::Attacker;
    let mut p2_pos = Vec3::ZERO;
    let mut p2_role = PlayerRole::Defender;
    
    for (_, id, role, transform) in player_query.iter() {
        match id {
            PlayerId::Player1 => {
                p1_pos = transform.translation;
                p1_role = *role;
            }
            PlayerId::Player2 => {
                p2_pos = transform.translation;
                p2_role = *role;
            }
        }
    }
    
    // 为玩家1创建人形sprite
    spawn_humanoid_sprite(
        &mut commands,
        PlayerId::Player1,
        Color::rgb(0.2, 0.4, 1.0),
        p1_pos,
        Some(p1_head_image.clone()),
        is_network_mode,
        current_player_id == PlayerId::Player1,
        current_player_is_attacker,
    );
    
    // 为玩家2创建人形sprite
    spawn_humanoid_sprite(
        &mut commands,
        PlayerId::Player2,
        Color::rgb(0.2, 1.0, 0.4),
        p2_pos,
        Some(p2_head_image.clone()),
        is_network_mode,
        current_player_id == PlayerId::Player2,
        current_player_is_attacker,
    );
    
    // 创建墙体
    let brick_cols = BRICK_COLS;
    let brick_rows = BRICK_ROWS;
    let brick_width = BRICK_WIDTH;
    let brick_height = BRICK_HEIGHT;
    let brick_colors = vec![Color::rgb(0.8, 0.6, 0.5), Color::rgb(0.5, 0.35, 0.25)];
    let wall_entity = commands.spawn((gameplay::Wall { damaged: false, damage_positions: Vec2::ZERO },)).id();
    
    // 确定要创建的视角
    let view_layers_to_create: Vec<ViewLayer> = if current_player_is_attacker {
        vec![ViewLayer::AttackerView]
    } else {
        vec![ViewLayer::DefenderView]
    };
    
    for row in 0..brick_rows {
        for col in 0..brick_cols {
            let x_offset = (col as f32 - (brick_cols as f32 - 1.0) / 2.0) * brick_width;
            let y_offset = (row as f32 - (brick_rows as f32 - 1.0) / 2.0) * brick_height;
            let row_offset = if row % 2 == 1 { brick_width / 2.0 } else { 0.0 };
            let final_x_offset = x_offset + row_offset;
            let brick_color = brick_colors[(col + row) % brick_colors.len()];
            
            for view_layer in view_layers_to_create.iter() {
                let render_layer = match view_layer {
                    ViewLayer::AttackerView => RenderLayers::layer(0),
                    ViewLayer::DefenderView => RenderLayers::layer(1),
                };
                
                let wall_z_pos = match view_layer {
                    ViewLayer::AttackerView => 2.0,
                    ViewLayer::DefenderView => 1.0,
                };
                
                if matches!(view_layer, ViewLayer::AttackerView) {
                    let background_z = wall_z_pos - 0.1;
                    commands.spawn((
                        gameplay::WallBackground {
                            position: Vec2::new(final_x_offset, y_offset),
                            view_layer: *view_layer,
                        },
                        SpriteBundle {
                            sprite: Sprite {
                                color: Color::BLACK,
                                custom_size: Some(Vec2::new(brick_width, brick_height)),
                                ..default()
                            },
                            transform: Transform::from_translation(Vec3::new(
                                final_x_offset,
                                WALL_POSITION.y + y_offset,
                                background_z,
                            )),
                            ..default()
                        },
                        render_layer,
                    ));
                }
                
                // 检查是否需要恢复破碎状态
                let segment_pos = Vec2::new(final_x_offset, y_offset);
                let is_broken = if let Some(broken_data) = broken_wall_data.as_ref() {
                    let is_host = room_info.as_ref().unwrap().is_host;
                    if is_host {
                        broken_data.host_broken_segments.contains(&segment_pos)
                    } else {
                        broken_data.client_broken_segments.contains(&segment_pos)
                    }
                } else {
                    false
                };
                
                let (segment_damaged, segment_color, segment_visibility) = if is_broken {
                    // 恢复破碎状态
                    // 调试输出已禁用: println!("[恢复破碎墙体] 位置: {:?}, 视角: {:?}", segment_pos, view_layer);
                    match view_layer {
                        ViewLayer::AttackerView => (true, Color::rgba(0.0, 0.0, 0.0, 0.0), Visibility::Hidden),
                        ViewLayer::DefenderView => (true, Color::rgba(0.0, 0.0, 0.0, 0.8), Visibility::Visible),
                    }
                } else {
                    (false, brick_color, Visibility::Visible)
                };
                
                let segment_entity = commands.spawn((
                    gameplay::WallSegment {
                        wall_entity,
                        position: segment_pos,
                        damaged: segment_damaged,
                        view_layer: *view_layer,
                    },
                    SpriteBundle {
                        sprite: Sprite {
                            color: segment_color,
                            custom_size: Some(Vec2::new(brick_width - 2.0, brick_height - 2.0)),
                            ..default()
                        },
                        transform: Transform::from_translation(Vec3::new(
                            final_x_offset, WALL_POSITION.y + y_offset, wall_z_pos
                        )),
                        visibility: segment_visibility,
                        ..default()
                    },
                    Collider { size: Vec2::new(brick_width - 2.0, brick_height - 2.0) },
                    render_layer,
                )).id();
                
                // 如果是防守方视角的破碎墙体，创建破损效果
                if is_broken && matches!(view_layer, ViewLayer::DefenderView) {
                    commands.spawn((
                        SpriteBundle {
                            sprite: Sprite { color: Color::rgba(0.0, 0.0, 0.0, 0.9), custom_size: Some(Vec2::new(30.0, 30.0)), ..default() },
                            transform: Transform::from_translation(Vec3::new(
                                final_x_offset, WALL_POSITION.y + y_offset, wall_z_pos + 0.5
                            )),
                            ..default()
                        },
                        render_layer,
                    ));
                }
            }
        }
    }
    
    // 创建准星和激光指示器
    if current_player_is_attacker {
        // 进攻方：创建准星
        commands.spawn((
            Crosshair,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(2.0, 30.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                ..default()
            },
            RenderLayers::layer(0),
        ));
        commands.spawn((
            Crosshair,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(30.0, 2.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                ..default()
            },
            RenderLayers::layer(0),
        ));
        commands.spawn((
            Crosshair,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(4.0, 4.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 303.0)),
                ..default()
            },
            RenderLayers::layer(0),
        ));
    } else {
        // 防守方：创建激光指示器和准星指示器
        commands.spawn((
            gameplay::LaserIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgba(1.0, 0.0, 0.0, 0.8), custom_size: Some(Vec2::new(100.0, 3.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 50.0)),
                ..default()
            },
            RenderLayers::layer(1),
        ));
        commands.spawn((
            DefenderCrosshairIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(2.0, 30.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                ..default()
            },
            RenderLayers::layer(1),
        ));
        commands.spawn((
            DefenderCrosshairIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(30.0, 2.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 302.0)),
                ..default()
            },
            RenderLayers::layer(1),
        ));
        commands.spawn((
            DefenderCrosshairIndicator,
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.0, 0.0), custom_size: Some(Vec2::new(4.0, 4.0)), ..default() },
                transform: Transform::from_translation(Vec3::new(0.0, 0.0, 303.0)),
                ..default()
            },
            RenderLayers::layer(1),
        ));
    }
    
    // 调试输出已禁用: println!("[角色切换] ========== 游戏实体重建完成 ==========");
}

/// 网络模式：更新UI显示/隐藏（角色切换时）
/// 注意：只更新根节点，子节点会自动继承父节点的显示状态
fn update_network_ui_visibility(
    mut queries: ParamSet<(
        Query<&mut Style, With<AttackerUIRoot>>,
        Query<&mut Style, With<DefenderUIRoot>>,
    )>,
    _view_config: Res<ViewConfig>,
    room_info: Option<Res<RoomInfo>>,
    app_state: Res<State<AppState>>,
    // 添加一个额外的查询来检查防守方UI根节点是否存在
    defender_root_check: Query<Entity, With<DefenderUIRoot>>,
    player_query: Query<(&crate::PlayerId, &crate::PlayerRole)>,
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    // 在GameOver状态下，确保防守方UI可见（这样防守方能看到最终状态）
    let is_game_over = *app_state == AppState::GameOver;
    
    // 根据本地玩家的实际角色来确定显示状态，确保准确性
    let room_info = room_info.unwrap();
    let local_player_id = if room_info.is_host {
        crate::PlayerId::Player1
    } else {
        crate::PlayerId::Player2
    };
    
    let mut local_role: Option<crate::PlayerRole> = None;
    for (player_id, role) in player_query.iter() {
        if *player_id == local_player_id {
            local_role = Some(*role);
            break;
        }
    }
    
    // 优先使用实际角色，如果找不到则根据 room_info.is_host 来判断
    // 房主（is_host=true）是进攻方，客户端（is_host=false）是防守方
    let is_attacker_view = if let Some(role) = local_role {
        matches!(role, crate::PlayerRole::Attacker)
    } else {
        // 如果找不到本地玩家角色，根据 room_info.is_host 来判断
        // 房主是进攻方，客户端是防守方
        let fallback_is_attacker = room_info.is_host;
        // 调试输出已禁用: println!("[警告] 未找到本地玩家角色，使用 room_info.is_host = {} 来判断（房主=进攻方，客户端=防守方）", room_info.is_host);
        fallback_is_attacker
    };
    
    // 检查防守方UI根节点是否存在
    let defender_root_exists = !defender_root_check.is_empty();
    if !defender_root_exists {
        // 调试输出已禁用: println!("[警告] 防守方UI根节点不存在！检查实体数量: {}", defender_root_check.iter().count());
    }
    
    // 更新进攻方UI显示（只更新根节点，子节点会自动继承）
    let mut attacker_root_count = 0;
    queries.p0().iter_mut().for_each(|mut style| {
        attacker_root_count += 1;
        // 在GameOver状态下，进攻方UI也可见（显示最终状态）
        style.display = if is_attacker_view || is_game_over { Display::Flex } else { Display::None };
    });
    
    // 更新防守方UI显示（只更新根节点，子节点会自动继承）
    // 与进攻方UI完全一致：只通过display控制显示/隐藏
    let mut defender_root_count = 0;
    let is_defender = !is_attacker_view; // 防守方视角 = 不是进攻方视角
    let should_display = is_defender || is_game_over; // 防守方必须显示，或者游戏结束时也显示
    let new_display = if should_display { Display::Flex } else { Display::None };
    
    // 使用ParamSet中的查询来更新Style（与进攻方UI完全一致）
    for mut style in queries.p1().iter_mut() {
        defender_root_count += 1;
        let old_display = style.display.clone();
        style.display = new_display;
        // 调试信息（游戏结束时只打印一次，避免每帧输出）
        // if is_game_over {
        //     println!("[update_network_ui_visibility] 游戏结束时防守方UI: display {} -> {} (is_attacker_view={}, is_defender={}, should_display={}, is_game_over={}, local_player_id={:?}, local_role={:?})", 
        //              if old_display == Display::Flex { "显示" } else { "隐藏" },
        //              if new_display == Display::Flex { "显示" } else { "隐藏" },
        //              is_attacker_view, is_defender, should_display, is_game_over, local_player_id, local_role);
        // }
    }
    
    // 调试：如果防守方UI没有找到，打印警告（只在第一次或游戏结束时打印，避免每帧输出）
    // if defender_root_count == 0 {
    //     println!("[update_network_ui_visibility] 警告：未找到防守方UI根节点！");
    //     println!("[update_network_ui_visibility] 当前视图配置: is_attacker_view = {}", is_attacker_view);
    //     println!("[update_network_ui_visibility] 找到的进攻方UI根节点数: {}", attacker_root_count);
    //     println!("[update_network_ui_visibility] 找到的防守方UI根节点数: {}", defender_root_count);
    //     println!("[update_network_ui_visibility] 防守方UI根节点实体存在（通过额外查询）: {}", defender_root_exists);
    //     if defender_root_exists {
    //         println!("[update_network_ui_visibility] 警告：防守方UI根节点存在但查询找不到，可能是查询条件问题！");
    //     }
    // } else if is_game_over {
    //     println!("[update_network_ui_visibility] 游戏结束时找到 {} 个防守方UI根节点", defender_root_count);
    // }
}

/// 强制显示防守方UI（确保防守方UI始终可见）
/// 这个系统在网络模式下，当玩家是防守方时，强制设置display和visibility
fn force_defender_ui_visible(
    mut commands: Commands,
    mut queries: ParamSet<(
        Query<&mut Style, With<AttackerUIRoot>>,
        Query<(Entity, &mut Style), With<DefenderUIRoot>>,
    )>,
    ui_camera_query: Query<Entity, With<IsDefaultUiCamera>>,
    room_info: Option<Res<RoomInfo>>,
    player_query: Query<(&crate::PlayerId, &crate::PlayerRole)>,
    app_state: Res<State<AppState>>,
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    // 只在Playing或GameOver状态下执行
    let is_playing_or_gameover = matches!(*app_state.get(), AppState::Playing | AppState::GameOver);
    if !is_playing_or_gameover {
        return;
    }
    
    let room_info = room_info.unwrap();
    let local_player_id = if room_info.is_host {
        crate::PlayerId::Player1
    } else {
        crate::PlayerId::Player2
    };
    
    // 确定本地玩家的角色
    let mut local_role: Option<crate::PlayerRole> = None;
    for (player_id, role) in player_query.iter() {
        if *player_id == local_player_id {
            local_role = Some(*role);
            break;
        }
    }
    
    // 如果找不到本地玩家角色，根据 room_info.is_host 来判断
    let is_defender = if let Some(role) = local_role {
        matches!(role, crate::PlayerRole::Defender)
    } else {
        !room_info.is_host // 客户端是防守方
    };
    
    // 获取UI相机实体
    let ui_camera_entity = ui_camera_query.get_single().ok();
    
    // 只在防守方玩家时强制显示防守方UI，进攻方玩家时强制隐藏
    // 确保防守方UI只在防守方窗口显示，不在进攻方窗口显示
    let mut found_count = 0;
    for (entity, mut style) in queries.p1().iter_mut() {
        found_count += 1;
        let old_display = style.display.clone();
        let should_display = is_defender; // 只在防守方玩家时显示
        
        // 根据is_defender设置display
        style.display = if should_display { Display::Flex } else { Display::None };
        
        // 确保visibility是Visible
        commands.entity(entity).insert(Visibility::Visible);
        
        // 打印详细信息用于调试（只在状态改变时打印）
        if old_display != style.display {
            println!("[强制显示] 防守方UI状态: display {:?} -> {} (is_defender={}, local_player_id={:?}, local_role={:?}, room_info.is_host={})", 
                     old_display, if should_display { "Flex" } else { "None" }, is_defender, local_player_id, local_role, room_info.is_host);
            println!("[强制显示] 防守方UI Style详情: width={:?}, height={:?}, position_type={:?}, overflow={:?}", 
                     style.width, style.height, style.position_type, style.overflow);
            if let Some(ui_camera) = ui_camera_entity {
                // 调试输出已禁用: println!("[强制显示] UI相机实体: {:?}", ui_camera);
            }
        }
    }
    if found_count == 0 {
        println!("[强制显示] 错误：未找到防守方UI根节点！(is_defender={}, local_player_id={:?}, local_role={:?}, room_info.is_host={})", 
                 is_defender, local_player_id, local_role, room_info.is_host);
    }
}

/// 调试系统：检查UI相机和防守方UI的渲染关系
fn debug_ui_camera_and_defender_ui(
    ui_camera_query: Query<(Entity, &Camera, Option<&RenderLayers>), With<IsDefaultUiCamera>>,
    defender_ui_root_query: Query<Entity, With<DefenderUIRoot>>,
    parent_query: Query<&Parent>,
    children_query: Query<&Children>,
    style_query: Query<&Style>,
    visibility_query: Query<&Visibility>,
    room_info: Option<Res<RoomInfo>>,
    app_state: Res<State<AppState>>,
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    // 只在Playing或GameOver状态下执行
    let is_playing_or_gameover = matches!(*app_state.get(), AppState::Playing | AppState::GameOver);
    if !is_playing_or_gameover {
        return;
    }
    
    // 只在前几次执行时打印（避免日志过多）
    static mut CALL_COUNT: u32 = 0;
    unsafe {
        CALL_COUNT += 1;
        if CALL_COUNT > 3 {
            return;
        }
    }
    
    // 检查UI相机
    let ui_camera_count = ui_camera_query.iter().count();
    // 调试输出已禁用: println!("[调试UI相机] 找到 {} 个UI相机（IsDefaultUiCamera）", ui_camera_count);
    for (entity, camera, render_layers) in ui_camera_query.iter() {
        // 调试输出已禁用: println!("[调试UI相机] UI相机实体: {:?}, order: {}, is_active: {}, target: {:?}, RenderLayers: {:?}", 
        //              entity, camera.order, camera.is_active, camera.target, render_layers);
    }
    
    // 检查防守方UI根节点及其父节点
    for root_entity in defender_ui_root_query.iter() {
        // 调试输出已禁用: println!("[调试UI相机] 防守方UI根节点: {:?}", root_entity);
        
        // 检查防守方UI根节点的Style和Visibility
        if let Ok(style) = style_query.get(root_entity) {
            // 调试输出已禁用: println!("[调试UI相机] 防守方UI根节点Style: display={:?}, width={:?}, height={:?}", 
            //              style.display, style.width, style.height);
        }
        if let Ok(visibility) = visibility_query.get(root_entity) {
            // 调试输出已禁用: println!("[调试UI相机] 防守方UI根节点Visibility: {:?}", visibility);
        }
        
        // 检查父节点（最外层UI根节点）
        if let Ok(parent) = parent_query.get(root_entity) {
            let parent_entity = parent.get();
            // 调试输出已禁用: println!("[调试UI相机] 防守方UI根节点的父节点: {:?}", parent_entity);
            
            // 检查父节点的Style和Visibility
            if let Ok(parent_style) = style_query.get(parent_entity) {
                // 调试输出已禁用: println!("[调试UI相机] 父节点Style: display={:?}, width={:?}, height={:?}", 
                //              parent_style.display, parent_style.width, parent_style.height);
            }
            if let Ok(parent_visibility) = visibility_query.get(parent_entity) {
                // 调试输出已禁用: println!("[调试UI相机] 父节点Visibility: {:?}", parent_visibility);
            }
            
            // 检查父节点的子节点（应该包含防守方UI根节点）
            if let Ok(parent_children) = children_query.get(parent_entity) {
                // 调试输出已禁用: println!("[调试UI相机] 父节点有 {} 个子节点", parent_children.len());
                for (i, child) in parent_children.iter().enumerate() {
                    if let Ok(child_style) = style_query.get(*child) {
                        // 调试输出已禁用: println!("[调试UI相机] 父节点的子节点 {}: entity={:?}, display={:?}", 
                        //              i, child, child_style.display);
                    }
                }
            }
        } else {
            // 调试输出已禁用: println!("[调试UI相机] 警告：防守方UI根节点没有父节点！");
        }
    }
}

/// 调试系统：检查防守方UI的所有子元素
fn debug_defender_ui_children(
    defender_ui_root_query: Query<Entity, With<DefenderUIRoot>>,
    children_query: Query<&Children>,
    style_query: Query<&Style>,
    visibility_query: Query<&Visibility>,
    room_info: Option<Res<RoomInfo>>,
    app_state: Res<State<AppState>>,
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    // 只在Playing或GameOver状态下执行
    let is_playing_or_gameover = matches!(*app_state.get(), AppState::Playing | AppState::GameOver);
    if !is_playing_or_gameover {
        return;
    }
    
    // 只在前几次执行时打印（避免日志过多）
    static mut CALL_COUNT: u32 = 0;
    unsafe {
        CALL_COUNT += 1;
        if CALL_COUNT > 5 {
            return;
        }
    }
    
    for root_entity in defender_ui_root_query.iter() {
        // 调试输出已禁用: println!("[调试防守方UI] 找到防守方UI根节点: {:?}", root_entity);
        
        // 检查根节点的Style和Visibility
        if let Ok(style) = style_query.get(root_entity) {
            // 调试输出已禁用: println!("[调试防守方UI] 根节点Style: display={:?}, width={:?}, height={:?}", 
            //              style.display, style.width, style.height);
        }
        if let Ok(visibility) = visibility_query.get(root_entity) {
            // 调试输出已禁用: println!("[调试防守方UI] 根节点Visibility: {:?}", visibility);
        }
        
        // 检查子元素
        if let Ok(children) = children_query.get(root_entity) {
            // 调试输出已禁用: println!("[调试防守方UI] 根节点有 {} 个子元素", children.len());
            for (i, child) in children.iter().enumerate() {
                if let Ok(child_style) = style_query.get(*child) {
                    // 调试输出已禁用: println!("[调试防守方UI] 子元素 {}: entity={:?}, display={:?}, width={:?}, height={:?}", 
                    //              i, child, child_style.display, child_style.width, child_style.height);
                }
                if let Ok(child_visibility) = visibility_query.get(*child) {
                    // 调试输出已禁用: println!("[调试防守方UI] 子元素 {} Visibility: {:?}", i, child_visibility);
                }
            }
        } else {
            // 调试输出已禁用: println!("[调试防守方UI] 警告：根节点没有子元素！");
        }
    }
}

/// 在UI创建后立即更新一次显示状态（只在OnEnter时运行一次）
fn update_network_ui_visibility_once(
    mut queries: ParamSet<(
        Query<&mut Style, With<AttackerUIRoot>>,
        Query<&mut Style, With<DefenderUIRoot>>,
    )>,
    _view_config: Res<ViewConfig>,
    room_info: Option<Res<RoomInfo>>,
    player_query: Query<(&crate::PlayerId, &crate::PlayerRole)>,
    defender_check: Query<Entity, With<DefenderUIRoot>>, // 用于调试
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    // 根据本地玩家的实际角色来确定显示状态，而不是依赖 view_config
    // 因为 view_config 可能还没有正确设置
    let room_info = room_info.unwrap();
    let local_player_id = if room_info.is_host {
        crate::PlayerId::Player1
    } else {
        crate::PlayerId::Player2
    };
    
    let mut local_role: Option<crate::PlayerRole> = None;
    for (player_id, role) in player_query.iter() {
        if *player_id == local_player_id {
            local_role = Some(*role);
            break;
        }
    }
    
    // 如果找不到本地玩家角色，根据 room_info.is_host 来判断
    // 房主（is_host=true）是进攻方，客户端（is_host=false）是防守方
    let is_attacker_view = if let Some(role) = local_role {
        matches!(role, crate::PlayerRole::Attacker)
    } else {
        // 如果找不到本地玩家角色，根据 room_info.is_host 来判断
        // 房主是进攻方，客户端是防守方
        let fallback_is_attacker = room_info.is_host;
        // 调试输出已禁用: println!("[警告] 初始化时未找到本地玩家角色，使用 room_info.is_host = {} 来判断（房主=进攻方，客户端=防守方）", room_info.is_host);
        fallback_is_attacker
    };
    
    println!("[初始化] 本地玩家: {:?}, 角色: {:?}, is_attacker_view={}", 
             local_player_id, local_role, is_attacker_view);
    
    // 更新进攻方UI显示
    let mut attacker_count = 0;
    queries.p0().iter_mut().for_each(|mut style| {
        attacker_count += 1;
        style.display = if is_attacker_view { Display::Flex } else { Display::None };
    });
    
    // 更新防守方UI显示（与进攻方UI完全一致：只通过display控制）
    let mut defender_count = 0;
    let should_display = !is_attacker_view;
    let new_display = if should_display { Display::Flex } else { Display::None };
    
    for mut style in queries.p1().iter_mut() {
        defender_count += 1;
        let old_display = style.display.clone();
        style.display = new_display;
        println!("[初始化] 防守方UI根节点显示状态已设置: display {} -> {} (is_attacker_view={}, 基于角色: {:?})", 
                 if old_display == Display::Flex { "显示" } else { "隐藏" },
                 if should_display { "显示" } else { "隐藏" },
                 is_attacker_view, local_role);
        // 额外调试：检查实际的Style属性
        println!("[初始化] 防守方UI根节点Style详情: display={:?}, width={:?}, height={:?}, position_type={:?}", 
                 style.display, style.width, style.height, style.position_type);
    };
    
    // 额外检查：使用不同的查询来验证防守方UI根节点是否存在
    let defender_check_count = defender_check.iter().count();
    if defender_count == 0 {
        // 调试输出已禁用: println!("[警告] 初始化时未找到防守方UI根节点！");
        // 调试输出已禁用: println!("[调试] 通过额外查询找到 {} 个带有 DefenderUIRoot 和 DefenderUI 的实体", defender_check_count);
        if defender_check_count > 0 {
            // 调试输出已禁用: println!("[警告] 防守方UI根节点存在，但 ParamSet 查询找不到！可能是查询时机问题。");
            for entity in defender_check.iter() {
                // 调试输出已禁用: println!("[调试] 找到防守方UI根节点实体: {:?}", entity);
            }
        }
    } else {
        println!("[初始化] 找到 {} 个进攻方UI根节点，{} 个防守方UI根节点，已更新显示状态", 
                 attacker_count, defender_count);
        // 调试输出已禁用: println!("[调试] 额外查询确认：找到 {} 个防守方UI根节点实体", defender_check_count);
    }
}

/// 设置UI界面
/// 根据游戏模式决定是双窗口显示还是单窗口显示
fn setup_ui(commands: &mut Commands, font: Handle<Font>, room_info: Option<&RoomInfo>) {
    let text_style = TextStyle {
        font: font.clone(),
        font_size: 24.0, 
        color: Color::WHITE,
    };
    let title_text_style = TextStyle { 
        font: font.clone(),
        font_size: 20.0, 
        color: Color::WHITE,
    };
    
    // 判断是否为网络对战模式
    let is_network_mode = room_info.is_some() && room_info.unwrap().is_connected;
    let is_attacker = if is_network_mode {
        room_info.unwrap().is_host // 网络模式下，房主是进攻方
    } else {
        true // 本地模式默认显示双窗口
    };
    
    // 创建UI根节点（最外层容器）
    // 注意：这个节点必须始终显示，因为它包含所有的UI子元素
    // UI元素不设置RenderLayers，这样它们会被UI相机（RenderLayers::none()）渲染，不会被游戏相机渲染
    let ui_root_entity = commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                overflow: Overflow::clip(), // 裁剪超出边界的UI
                display: Display::Flex, // 确保最外层根节点始终显示
                ..default()
            },
            z_index: ZIndex::Global(0), // 最外层根节点使用最低的z_index，确保子节点可以覆盖它
            visibility: Visibility::Visible, // 明确设置visibility为Visible
            ..default()
        },
        // 不设置RenderLayers，这样UI元素会被UI相机（RenderLayers::none()）渲染
    )).id();
    // 调试输出已禁用: println!("[setup_ui] 创建最外层UI根节点，实体ID: {:?}, display=Flex, visibility=Visible", ui_root_entity);
    
    commands.entity(ui_root_entity).with_children(|parent| {
        if is_network_mode {
            // 网络模式：同时创建进攻方和防守方UI，但根据view_config显示/隐藏
            // 这样角色切换时就不需要重新创建UI了
            // 进攻方UI
            parent.spawn((
                NodeBundle {
            style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        position_type: PositionType::Absolute,
                        overflow: Overflow::clip(),
                        display: if is_attacker { Display::Flex } else { Display::None }, // 根据初始角色显示/隐藏
                ..default()
            },
            z_index: ZIndex::Global(100), // 进攻方UI使用较低的z_index
            ..default()
                },
                // 不设置RenderLayers，让UI元素使用默认的layer 0
                // UI元素会自动渲染到有IsDefaultUiCamera的相机
                AttackerUI,
                AttackerUIRoot, // 标记这是根节点
            )).with_children(|attacker_parent| {
                    setup_attacker_ui(attacker_parent, font.clone(), text_style.clone(), title_text_style.clone());
                });
            
            // 防守方UI
            // 网络模式下：始终创建防守方UI，但根据角色显示/隐藏
            // 注意：与进攻方UI完全一致的创建方式，只通过display控制显示/隐藏
            // 调试输出已禁用: println!("[setup_ui] 创建防守方UI (is_network_mode={}, is_attacker={})", is_network_mode, is_attacker);
            // 设置更高的z_index，确保防守方UI在最后渲染（在所有其他UI之上）
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        position_type: PositionType::Absolute,
                        overflow: Overflow::clip(),
                        display: if !is_attacker { Display::Flex } else { Display::None }, // 与进攻方UI完全一致：根据初始角色显示/隐藏
                        ..default()
                    },
                    z_index: ZIndex::Global(9999), // 设置非常高的z_index，确保防守方UI在最后渲染，不会被任何其他UI遮挡
                    visibility: Visibility::Visible, // 明确设置visibility为Visible（NodeBundle已经包含Visibility组件，直接设置字段即可）
                    ..default()
                },
                // 不设置RenderLayers，让UI元素使用默认的layer 0
                // UI元素会自动渲染到有IsDefaultUiCamera的相机
                DefenderUI,
                DefenderUIRoot, // 标记这是根节点
            )).with_children(|defender_parent| {
                    // 不再创建测试UI，直接创建防守方UI
                    let _child_count = setup_defender_ui(defender_parent, font.clone(), text_style.clone(), title_text_style.clone());
                    // 调试输出已禁用: println!("[调试] 防守方UI子元素创建完成，共创建 {} 个子元素", _child_count);
                }).id();
            println!("[调试] 创建防守方UI根节点, is_attacker={}, 初始display={}", 
                     is_attacker, if !is_attacker { "Flex" } else { "None" });
            // 调试输出已禁用: println!("[调试] 最外层UI根节点实体ID: {:?}", ui_root_entity);
        } else {
            // 本地模式：显示双窗口UI
            // 左侧视角（进攻方）的UI容器 - 只占屏幕左半部分（0-50%）
            parent.spawn(NodeBundle {
                style: Style {
                    width: Val::Percent(50.0),
                    height: Val::Percent(100.0),
                    left: Val::Percent(0.0),
                    position_type: PositionType::Absolute,
                    overflow: Overflow::clip(),
                    ..default()
                },
                ..default()
            }).with_children(|attacker_parent| {
                setup_attacker_ui(attacker_parent, font.clone(), text_style.clone(), title_text_style.clone());
            });
            
            // 右侧视角（防守方）的UI容器 - 只占屏幕右半部分（50-100%）
            parent.spawn(NodeBundle {
                style: Style {
                    width: Val::Percent(50.0),
                    height: Val::Percent(100.0),
                    left: Val::Percent(50.0),
                    position_type: PositionType::Absolute,
                    overflow: Overflow::clip(),
                    ..default()
                },
                ..default()
            }).with_children(|defender_parent| {
                setup_defender_ui(defender_parent, font.clone(), text_style.clone(), title_text_style.clone());
            });
        }
        
        // 操作提示（设置较低的z_index，确保不会遮挡防守方UI）
        parent.spawn(NodeBundle {
            style: Style {
                width: Val::Percent(100.0), bottom: Val::Px(20.0),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                ..default()
            },
            z_index: ZIndex::Global(50), // 设置较低的z_index，确保不会遮挡防守方UI（z_index=1000）
            ..default()
        }).with_children(|bottom| {
            bottom.spawn(TextBundle::from_sections([
                TextSection::new("他们走不了了！".to_string(), text_style.clone()),
            ]));
        });
    });
}

/// 设置进攻方视角的UI（左侧视口）
fn setup_attacker_ui(parent: &mut ChildBuilder, font: Handle<Font>, _text_style: TextStyle, title_text_style: TextStyle) {
    // 血量显示（显示在左侧视口顶部）
    parent.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0), // 占满父容器（即左半屏幕）
                top: Val::Px(0.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Row, 
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center, 
                padding: UiRect::all(Val::Px(10.0)),
                        ..default()
                    },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(), // 全透明
            z_index: ZIndex::Global(100),
                    ..default()
                },
        AttackerUI,
    )).with_children(|attacker_hp_bar| {
        // P1血量
        attacker_hp_bar.spawn(NodeBundle {
            style: Style { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(5.0)), ..default() },
            background_color: Color::rgba(0.1, 0.3, 0.8, 0.8).into(), // 全透明
            ..default()
        }).with_children(|hp_bar| {
            hp_bar.spawn((
                TextBundle::from_sections([
                    TextSection::new("P1 血量: ".to_string(), title_text_style.clone()),
                    TextSection::new("100".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::WHITE }),
                ]),
                PlayerHealthDisplay { player_id: PlayerId::Player1 },
                AttackerUI,
            ));
        });
        // P2血量
        attacker_hp_bar.spawn(NodeBundle {
            style: Style { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(5.0)), ..default() },
            background_color: Color::rgba(0.8, 0.1, 0.1, 0.8).into(), // 全透明
            ..default()
        }).with_children(|hp_bar| {
            hp_bar.spawn((
                TextBundle::from_sections([
                    TextSection::new("P2 血量: ".to_string(), title_text_style.clone()),
                    TextSection::new("100".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::WHITE }),
                ]),
                PlayerHealthDisplay { player_id: PlayerId::Player2 },
                AttackerUI,
            ));
        });
    });
    
    // 剩余子弹数（显示在左侧视口）
    parent.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0), // 占满父容器（即左半屏幕）
                top: Val::Px(70.0),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center, 
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column, 
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(), // 全透明
            z_index: ZIndex::Global(100),
            ..default()
        },
        AttackerUI,
    )).with_children(|center| {
        center.spawn(TextBundle::from_sections([
            TextSection::new("剩余子弹: ".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::WHITE }),
        ]));
        center.spawn(NodeBundle {
            style: Style { flex_direction: FlexDirection::Row, margin: UiRect::top(Val::Px(5.0)), ..default() },
            ..default()
        }).with_children(|bullets_container| {
            for i in 0..BULLETS_PER_ROUND {
                bullets_container.spawn((
                    TextBundle::from_sections([
                        TextSection::new("● ".to_string(), TextStyle { font: font.clone(), font_size: 30.0, color: Color::rgb(1.0, 0.84, 0.0) }),
                    ]),
                    BulletIcon { index: i as usize },
                    AttackerUI,
                ));
            }
        });
    });
    
    // 时间文本（显示在左侧视口，调整位置避免与子弹UI重叠）
    parent.spawn((
        TextBundle {
            text: Text::from_sections([
                TextSection::new("剩余时间: ".to_string(), TextStyle { font: font.clone(), font_size: 20.0, color: Color::WHITE }),
                TextSection::new(ROUND_TIME_SECONDS.to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::YELLOW }),
            ]),
            style: Style {
                position_type: PositionType::Absolute, 
                top: Val::Px(140.0), // 调整位置，避免与子弹UI重叠（子弹UI在70px，时间在140px）
                left: Val::Percent(50.0), // 左侧视口的中心（相对于父容器）
                ..default()
            },
            z_index: ZIndex::Global(100),
            transform: Transform::from_xyz(-60.0, 0.0, 0.0),
            ..default()
        },
        TimerText,
        AttackerUI,
    ));
}

/// 设置防守方视角的UI（右侧视口）
/// 返回创建的子元素数量
fn setup_defender_ui(parent: &mut ChildBuilder, font: Handle<Font>, text_style: TextStyle, title_text_style: TextStyle) -> usize {
    // 血量显示（显示在右侧视口顶部）
    parent.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0), // 占满父容器（即右半屏幕）
                top: Val::Px(0.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Row, 
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center, 
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(), // 全透明
            z_index: ZIndex::Global(9999), // 防守方UI使用非常高的z_index，确保最后渲染，不会被遮挡
            ..default()
        },
        DefenderUI,
    )).with_children(|defender_hp_bar| {
        // P1血量
        defender_hp_bar.spawn(NodeBundle {
            style: Style { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(5.0)), ..default() },
            background_color: Color::rgba(0.1, 0.3, 0.8, 0.8).into(), // 全透明
            ..default()
        }).with_children(|hp_bar| {
            hp_bar.spawn((
                TextBundle::from_sections([
                    TextSection::new("P1 血量: ".to_string(), title_text_style.clone()),
                    TextSection::new("100".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::WHITE }),
                ]),
                PlayerHealthDisplay { player_id: PlayerId::Player1 },
                DefenderUI,
            ));
        });
        // P2血量
        defender_hp_bar.spawn(NodeBundle {
            style: Style { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(5.0)), ..default() },
            background_color: Color::rgba(0.8, 0.1, 0.1, 0.8).into(), // 全透明
            ..default()
        }).with_children(|hp_bar| {
            hp_bar.spawn((
                TextBundle::from_sections([
                    TextSection::new("P2 血量: ".to_string(), title_text_style.clone()),
                    TextSection::new("100".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::WHITE }),
                ]),
                PlayerHealthDisplay { player_id: PlayerId::Player2 },
                DefenderUI,
            ));
        });
    });
    
    // 剩余子弹数（显示在右侧视口）
    parent.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0), // 占满父容器（即右半屏幕）
                top: Val::Px(70.0),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center, 
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column, 
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(), // 全透明
            z_index: ZIndex::Global(9999), // 防守方UI使用非常高的z_index，确保最后渲染，不会被遮挡
            ..default()
        },
        DefenderUI,
    )).with_children(|center| {
        center.spawn(TextBundle::from_sections([
            TextSection::new("剩余子弹: ".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::WHITE }),
        ]));
        center.spawn(NodeBundle {
            style: Style { flex_direction: FlexDirection::Row, margin: UiRect::top(Val::Px(5.0)), ..default() },
            ..default()
        }).with_children(|bullets_container| {
            for i in 0..BULLETS_PER_ROUND {
                bullets_container.spawn((
                    TextBundle::from_sections([
                        TextSection::new("● ".to_string(), TextStyle { font: font.clone(), font_size: 30.0, color: Color::rgb(1.0, 0.84, 0.0) }),
                    ]),
                    BulletIcon { index: i as usize },
                    DefenderUI,
                ));
            }
        });
    });
    
    // 剩余时间（显示在右侧视口）
    parent.spawn((
        TextBundle {
            text: Text::from_sections([
                TextSection::new("剩余时间: ".to_string(), TextStyle { font: font.clone(), font_size: 20.0, color: Color::WHITE }),
                TextSection::new(ROUND_TIME_SECONDS.to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::YELLOW }),
            ]),
            style: Style {
                position_type: PositionType::Absolute, 
                top: Val::Px(140.0), // 调整位置，避免与子弹UI重叠
                left: Val::Percent(50.0), // 右侧视口的中心（相对于父容器）
                ..default()
            },
            z_index: ZIndex::Global(9999), // 防守方UI使用非常高的z_index，确保最后渲染，不会被遮挡
            transform: Transform::from_xyz(-60.0, 0.0, 0.0),
            ..default()
        },
        TimerText,
        DefenderUI,
    ));
    
    // 动作冷却提示（显示在右侧视口）
    parent.spawn((
        TextBundle {
            text: Text::from_sections([
                TextSection::new("动作冷却: ".to_string(), text_style.clone()),
                TextSection::new("就绪".to_string(), TextStyle { font: font.clone(), font_size: 24.0, color: Color::GREEN }),
            ]),
            style: Style {
                position_type: PositionType::Absolute, 
                bottom: Val::Px(70.0), 
                left: Val::Percent(50.0), // 右侧视口的中心（相对于父容器）
                ..default()
            },
            z_index: ZIndex::Global(9999), // 防守方UI使用非常高的z_index，确保最后渲染，不会被遮挡
            transform: Transform::from_xyz(-100.0, 0.0, 0.0),
            ..default()
        },
        ActionCooldownText,
        DefenderUI,
    ));
    
    // 返回创建的直接子元素数量（4个：血量条、子弹数、时间、动作冷却）
    4
}

// --- 游玩系统函数已移至 gameplay.rs ---

// --- 图片加载检查系统已移至 gameplay.rs ---

// --- 所有游戏系统已移至 gameplay.rs ---
// --- main.rs 现在只包含主程序入口、游戏初始化和 UI 设置函数 ---
// --- main.rs 现在只包含主程序入口、游戏初始化和 UI 设置函数 ---