use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowResized},
    render::view::RenderLayers,
    render::camera::Viewport,
    math::UVec2,
    ecs::system::ParamSet,
};
use rand::Rng;
use std::collections::HashMap;
use crate::{
    PlayerId, PlayerRole, AppState, RoundState,
    PLAYER_SIZE, WALL_SIZE, WALL_POSITION, DEFENDER_START_POS, ATTACKER_START_POS,
    BULLET_SIZE, BULLET_SPEED, MUZZLE_FLASH_DURATION, PLAYER_MOVE_SPEED,
    AIM_SPEED, MAX_AIM_OFFSET, SIDE_DODGE_DISTANCE, CROSSHAIR_DAMAGE_RANGE,
    DAMAGE_HEAD, DAMAGE_TORSO, DAMAGE_LEGS,
    ACTION_DURATION_SECONDS, BULLETS_PER_ROUND,
    BRICK_COLS, BRICK_ROWS, BRICK_WIDTH, BRICK_HEIGHT,
    BulletIcon, PlayerHealthDisplay, ActionCooldownText, TimerText,
};

// --- 游玩系统集定义 ---
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameplaySystems {
    InputSystems,    // 输入相关系统（瞄准、射击、移动）
    ActionSystems,   // 动作相关系统（下蹲、侧躲、计时器）
    ViewSystems,     // 视图相关系统（激光、准星、视口）
    LogicSystems,    // 逻辑相关系统（子弹、碰撞、回合）
    UISystems,       // UI 相关系统（血量、冷却、计时器显示）
    EventSystems,    // 事件处理系统（命中、游戏结束、动作事件）
}

// --- 游玩系统组件定义 ---
#[derive(Component, Debug)]
pub struct Health(pub f32);

#[derive(Component, Debug)]
pub struct ActionCooldown {
    pub last_action_time: f64,
    pub cooldown_duration: f64,
}

#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DodgeAction {
    #[default]
    None,
    Crouch,
    SideLeft,
    SideRight,
}

#[derive(Component, Debug)]
pub struct ActionTimer {
    pub timer: Timer,
    pub action: DodgeAction,
    pub player_id: PlayerId,
}

/// 游戏结束延迟资源
#[derive(Resource, Default)]
pub struct GameOverDelay {
    pub timer: Option<Timer>,
    pub winner_id: Option<PlayerId>,
    pub loser_id: Option<PlayerId>,
    /// 持续发送GameOver网络消息的计时器（3秒内，每0.1秒发送一次）
    pub network_send_timer: Option<Timer>,
    pub network_send_count: u32, // 已发送次数
}

/// UI状态跟踪资源（用于优化UI更新系统）
#[derive(Resource, Default)]
pub struct UiStateTracker {
    pub last_bullets_left: i32,
    pub last_p1_health: f32,
    pub last_p2_health: f32,
    pub last_cooldown_text: String,
    pub viewport_initialized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitboxType {
    Head,
    Torso,
    Legs,
}

#[derive(Component, Debug)]
pub struct Collider {
    pub size: Vec2,
}

#[derive(Component, Debug)]
pub struct Wall {
    pub damaged: bool,
    pub damage_positions: Vec2,
}

#[derive(Component, Debug)]
pub struct WallSegment {
    pub wall_entity: Entity,
    pub position: Vec2,
    pub damaged: bool,
    pub view_layer: ViewLayer,
}

#[derive(Component)]
pub struct WallBackground {
    pub position: Vec2,
    pub view_layer: ViewLayer,
}

#[derive(Component, Debug)]
pub struct Bullet {
    pub velocity: Vec2,
    pub start_pos: Vec2,
    pub target_pos: Vec2,
    pub owner: PlayerId,
}

/// 标记子弹的渲染层（用于同步两个渲染层的子弹位置）
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletRenderLayer {
    Layer0, // 进攻方视角
    Layer1, // 防守方视角
}

/// 子弹同步ID：同一颗子弹的两个副本共享这个ID
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BulletSyncId(pub u64);

#[derive(Component, Debug)]
pub struct MuzzleFlash {
    pub timer: Timer,
}

#[derive(Component)]
pub struct Crosshair;

#[derive(Component)]
pub struct DefenderCrosshairIndicator;

#[derive(Component, Debug)]
pub struct DefenderAI {
    pub direction: f32,
    pub move_timer: Timer,
}

#[derive(Component)]
pub struct HumanoidPart {
    pub player_id: PlayerId,
    pub part_type: HumanoidPartType,
    pub view_layer: ViewLayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewLayer {
    AttackerView,
    DefenderView,
}

#[derive(Debug, Clone, Copy)]
pub enum HumanoidPartType {
    Head,
    Torso,
    Legs,
}

#[derive(Component)]
pub struct LaserIndicator;

#[derive(Component, Debug)]
pub struct DefenderCamera;

/// 标记相机对应的玩家ID（用于本地双人模式）
#[derive(Component, Debug)]
pub struct PlayerCamera {
    pub player_id: PlayerId,
}

// --- 游玩系统资源定义 ---
/// 再来一局状态
#[derive(Resource, Default, Debug)]
pub struct RematchState {
    pub host_ready: bool,
    pub client_ready: bool,
}

#[derive(Resource, Debug)]
pub struct RoundInfo {
    pub bullets_left: i32,
    pub round_timer: Timer,
    pub current_attacker: PlayerId,
    pub p1_health: f32,
    pub p2_health: f32,
    pub bullets_fired_this_round: i32,
    pub bullets_hit_defender: i32,
    pub is_switching: bool,
}

#[derive(Resource, Default, Debug)]
pub struct CursorPosition(pub Vec2);

#[derive(Resource, Default, Debug)]
pub struct CrosshairOffset(pub Vec2);

/// 子弹ID计数器（用于生成唯一的子弹同步ID）
#[derive(Resource, Debug)]
pub struct BulletIdCounter(pub u64);

impl Default for BulletIdCounter {
    fn default() -> Self {
        BulletIdCounter(0)
    }
}

#[derive(Resource, Default, Debug)]
pub struct ViewConfig {
    pub is_attacker_view: bool,
    pub viewport_entity: Option<Entity>,
}

/// 缓存上次的角色状态，用于优化性能（避免每帧检查）
#[derive(Resource, Default, Debug)]
pub struct LastRoleState {
    pub last_local_role: Option<PlayerRole>,
    pub last_view_config: bool,
}

/// 缓存相机状态，用于优化性能（避免每帧检查）
#[derive(Resource, Default, Debug)]
pub struct CameraStateCache {
    pub last_game_camera_count: usize,
    pub last_ui_camera_count: usize,
    pub last_is_attacker_view: bool,
    pub needs_check: bool, // 标记是否需要检查（相机可能被添加/删除时设置为true）
}

/// 本地双人模式的相机资源
#[derive(Resource, Debug)]
pub struct LocalMultiplayerCameras {
    pub left_camera: Entity,
    pub right_camera: Entity,
}

// --- 游玩系统事件定义 ---
#[derive(Event, Debug)]
pub struct PlayerHitEvent {
    pub player_id: PlayerId,
    pub damage: f32,
    pub hitbox_type: HitboxType,
}

#[derive(Event, Debug)]
pub struct GameOverEvent {
    pub winner_id: PlayerId,
    pub loser_id: PlayerId,
}

#[derive(Event, Debug)]
pub struct PlayerActionEvent {
    pub player_id: PlayerId,
    pub action: DodgeAction,
}

#[derive(Event, Debug)]
pub struct CameraSwitchEvent {
    pub is_attacker_view: bool,
}

/// 直接使用 ViewConfig 中保存的相机实体ID切换相机配置
/// 用于在 Commands 修改还未生效时直接操作实体
pub fn apply_network_camera_view_direct(
    commands: &mut Commands,
    all_cameras: &Query<Entity, With<Camera2d>>,
    view_config: &mut crate::ViewConfig,
    new_is_attacker: bool,
) {
    let should_be_defender_camera = !new_is_attacker;
    
    // 调试输出已禁用: println!("[角色切换] ========== 开始直接切换画面到{}视图 ==========", if should_be_defender_camera { "防守方" } else { "进攻方" });
    
    // 使用 ViewConfig 中保存的相机实体ID，但需要验证实体是否仍然存在
    let camera_entity = if let Some(entity) = view_config.viewport_entity {
        // 验证实体是否仍然存在于查询中
        if all_cameras.iter().any(|e| e == entity) {
            entity
        } else {
            // 实体已不存在，使用第一个找到的相机
            if let Some(entity) = all_cameras.iter().next() {
                entity
            } else {
                // 调试输出已禁用: println!("[错误] apply_network_camera_view_direct 未找到任何相机！");
                return;
            }
        }
    } else {
        // 如果没有保存的实体ID，使用第一个找到的相机
        if let Some(entity) = all_cameras.iter().next() {
            entity
        } else {
            // 调试输出已禁用: println!("[错误] apply_network_camera_view_direct 未找到任何相机！");
            return;
        }
    };
    
    // 验证实体是否可以通过 Commands 访问（如果实体已被销毁，get_entity 会返回 None）
    if commands.get_entity(camera_entity).is_none() {
        // 实体不存在，尝试使用第一个找到的相机
        if let Some(entity) = all_cameras.iter().next() {
            // 更新 view_config 中的实体ID
            view_config.viewport_entity = Some(entity);
            // 递归调用，使用新的实体
            return apply_network_camera_view_direct(commands, all_cameras, view_config, new_is_attacker);
        } else {
            // 调试输出已禁用: println!("[错误] apply_network_camera_view_direct 无法访问相机实体，且没有备用相机！");
            return;
        }
    }
    
    // 调试输出已禁用: println!("[角色切换] 找到游戏相机实体: {:?}，开始直接切换", camera_entity);
    
    // 先移除可能存在的 PlayerId 和 PlayerCamera 组件
    commands.entity(camera_entity).remove::<PlayerId>();
    commands.entity(camera_entity).remove::<PlayerCamera>();
    
    // 1. 切换相机类型（DefenderCamera 组件）
    if should_be_defender_camera {
        commands.entity(camera_entity).insert(DefenderCamera);
        // 调试输出已禁用: println!("  - 添加 DefenderCamera 组件");
    } else {
        commands.entity(camera_entity).remove::<DefenderCamera>();
        // 调试输出已禁用: println!("  - 移除 DefenderCamera 组件");
    }
    
    // 1.5. 切换IsDefaultUiCamera组件（进攻方：游戏相机作为UI相机；防守方：单独的UI相机）
    // 注意：UI相机的创建/删除会在recreate_game_entities_on_role_switch_system中处理
    // 这里只需要切换游戏相机的IsDefaultUiCamera组件
    if should_be_defender_camera {
        // 防守方：移除游戏相机的IsDefaultUiCamera（防守方使用单独的UI相机）
        commands.entity(camera_entity).remove::<bevy::ui::IsDefaultUiCamera>();
        // 调试输出已禁用: println!("  - 移除游戏相机的IsDefaultUiCamera（防守方使用单独的UI相机）");
    } else {
        // 进攻方：添加IsDefaultUiCamera到游戏相机（进攻方使用游戏相机作为UI相机）
        commands.entity(camera_entity).insert(bevy::ui::IsDefaultUiCamera);
        // 调试输出已禁用: println!("  - 添加IsDefaultUiCamera到游戏相机（进攻方使用游戏相机作为UI相机）");
    }
    
    // 2. 切换相机位置
    if should_be_defender_camera {
        commands.entity(camera_entity).insert(Transform::from_translation(Vec3::new(
            crate::WALL_POSITION.x,
            crate::WALL_POSITION.y,
            1000.0,
        )));
        // 调试输出已禁用: println!("  - 相机位置: ({}, {}) - 墙的位置", crate::WALL_POSITION.x, crate::WALL_POSITION.y);
    } else {
        commands.entity(camera_entity).insert(Transform::from_translation(Vec3::new(
            crate::ATTACKER_START_POS.x,
            crate::ATTACKER_START_POS.y,
            1000.0,
        )));
        // 调试输出已禁用: println!("  - 相机位置: ({}, {}) - 进攻方起始位置", crate::ATTACKER_START_POS.x, crate::ATTACKER_START_POS.y);
    }
    
    // 3. 切换渲染层（确保相机只渲染对应的layer）
    if should_be_defender_camera {
        commands.entity(camera_entity).insert(RenderLayers::layer(1));
        // 调试输出已禁用: println!("  - 渲染层: layer(1) - 防守方视角，只渲染layer 1的实体");
    } else {
        commands.entity(camera_entity).insert(RenderLayers::layer(0));
        // 调试输出已禁用: println!("  - 渲染层: layer(0) - 进攻方视角，只渲染layer 0的实体");
    }
    
    // 4. 背景色和缩放需要通过修改 Camera 和 Projection 组件
    // 但由于无法在同一帧查询，我们使用 Commands 的 insert 来覆盖
    // 注意：insert 会覆盖现有组件，所以我们可以直接插入新的值
    
    // 切换背景色
    if should_be_defender_camera {
        commands.entity(camera_entity).insert(Camera {
            clear_color: ClearColorConfig::Custom(Color::rgb(0.2, 0.0, 0.2)),
            ..default()
        });
        // 调试输出已禁用: println!("  - 背景色: 紫色 (0.2, 0.0, 0.2)");
    } else {
        commands.entity(camera_entity).insert(Camera {
            clear_color: ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0)),
            ..default()
        });
        // 调试输出已禁用: println!("  - 背景色: 黑色 (0.0, 0.0, 0.0)");
    }
    
    // 切换缩放（使用 Commands 的 insert 会覆盖现有组件，但需要确保正确应用）
    // 注意：由于无法在同一帧查询 Projection，我们需要使用 insert 来覆盖
    // 但为了确保缩放正确应用，我们使用更明确的方式
    if should_be_defender_camera {
        commands.entity(camera_entity).insert(Projection::Orthographic(OrthographicProjection {
            scale: 1.5,
            near: -1000.0,
            far: 1000.0,
            ..default()
        }));
        // 调试输出已禁用: println!("  - 缩放: 1.5 (大视野)");
    } else {
        commands.entity(camera_entity).insert(Projection::Orthographic(OrthographicProjection {
            scale: 0.5,
            near: -1000.0,
            far: 1000.0,
            ..default()
        }));
        // 调试输出已禁用: println!("  - 缩放: 0.5 (小视野)");
    }
    
    // 调试输出已禁用: println!("[角色切换] ========== 直接画面切换完成 ==========");
}

/// 立即根据 `new_is_attacker` 切换网络模式下的唯一相机配置
/// 该函数会同时处理相机组件、位置、背景色、缩放以及渲染层
pub fn apply_network_camera_view(
    commands: &mut Commands,
    camera_query: &mut Query<(
        Entity,
        &mut Camera,
        &mut Transform,
        &mut Projection,
        Option<&DefenderCamera>,
        Option<&mut RenderLayers>,
    ), (With<Camera2d>, Without<PlayerId>)>,
    new_is_attacker: bool,
) {
    let mut found_camera = false;
    let should_be_defender_camera = !new_is_attacker;
    
    // 调试输出已禁用: println!( "[角色切换] ========== 开始切换画面到{}视图 ==========", if should_be_defender_camera { "防守方" } else { "进攻方" } );
    
    // 调试：先检查查询结果
    let query_count = camera_query.iter().count();
    // 调试输出已禁用: println!("[角色切换] 调试：相机查询 (With<Camera2d>, Without<PlayerId>) 找到 {} 个相机", query_count);
    
    // 如果查询失败，尝试使用更宽松的查询条件
    if query_count == 0 {
        // 调试输出已禁用: println!("[角色切换] 警告：查询失败，尝试使用 Commands 直接访问相机实体");
        // 这里我们不能直接查询，但可以在调用此函数之前移除 PlayerId 组件
        return; // 暂时返回，让调用者处理
    }
    
    for (entity, mut camera, mut transform, mut projection, _has_defender_camera, render_layers_opt) in
        camera_query.iter_mut()
    {
        // 跳过 UI 相机（order > 0）
        if camera.order > 0 {
            // 调试输出已禁用: println!("[角色切换] 跳过UI相机 (order: {})", camera.order);
            continue;
        }

        found_camera = true;
        // 调试输出已禁用: println!( "[角色切换] 找到游戏相机 (entity: {:?}, order: {})，开始切换", entity, camera.order );

        // 1. 切换相机类型（DefenderCamera 组件）
        if should_be_defender_camera {
            commands.entity(entity).insert(DefenderCamera);
            // 调试输出已禁用: println!("  - 添加 DefenderCamera 组件");
        } else {
            commands.entity(entity).remove::<DefenderCamera>();
            // 调试输出已禁用: println!("  - 移除 DefenderCamera 组件");
        }

        // 2. 切换相机位置
        if should_be_defender_camera {
            // 防守方：相机固定在墙的位置
            transform.translation = Vec3::new(crate::WALL_POSITION.x, crate::WALL_POSITION.y, 1000.0);
            // 调试输出已禁用: println!( "  - 相机位置: ({}, {}) - 墙的位置", crate::WALL_POSITION.x, crate::WALL_POSITION.y );
        } else {
            // 进攻方：相机在进攻方起始位置
            transform.translation =
                Vec3::new(crate::ATTACKER_START_POS.x, crate::ATTACKER_START_POS.y, 1000.0);
            // 调试输出已禁用: println!( "  - 相机位置: ({}, {}) - 进攻方起始位置", crate::ATTACKER_START_POS.x, crate::ATTACKER_START_POS.y );
        }

        // 3. 切换背景色
        if should_be_defender_camera {
            camera.clear_color = ClearColorConfig::Custom(Color::rgb(0.2, 0.0, 0.2));
            // 调试输出已禁用: println!("  - 背景色: 紫色 (0.2, 0.0, 0.2)");
        } else {
            camera.clear_color = ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0));
            // 调试输出已禁用: println!("  - 背景色: 黑色 (0.0, 0.0, 0.0)");
        }

        // 4. 切换缩放
        if let Projection::Orthographic(ref mut ortho) = *projection {
            if should_be_defender_camera {
                ortho.scale = 1.5;
                // 调试输出已禁用: println!("  - 缩放: 1.5 (大视野)");
            } else {
                ortho.scale = 0.5;
                // 调试输出已禁用: println!("  - 缩放: 0.5 (小视野)");
            }
        }

        // 5. 切换渲染层
        if should_be_defender_camera {
            if let Some(mut render_layers) = render_layers_opt {
                *render_layers = RenderLayers::layer(1);
            } else {
                commands.entity(entity).insert(RenderLayers::layer(1));
            }
            // 调试输出已禁用: println!("  - 渲染层: layer(1) - 防守方视角的墙");
        } else {
            if let Some(mut render_layers) = render_layers_opt {
                *render_layers = RenderLayers::layer(0);
            } else {
                commands.entity(entity).insert(RenderLayers::layer(0));
            }
            // 调试输出已禁用: println!("  - 渲染层: layer(0) - 进攻方视角的墙");
        }

        // 调试输出已禁用: println!("[角色切换] ========== 画面切换完成 ==========");
    }
    
    if !found_camera {
        // 调试输出已禁用: println!("[错误] apply_network_camera_view 未找到任何游戏相机！");
            // 调试输出已禁用: println!("[错误] 相机查询结果（使用 With<Camera2d>, Without<PlayerId> 条件）：");
        let mut count = 0;
        for (entity, camera, _, _, _, _) in camera_query.iter() {
            // 调试输出已禁用: println!("  - 相机 entity: {:?}, order: {}", entity, camera.order);
            count += 1;
        }
        if count == 0 {
            // 调试输出已禁用: println!("[错误] 查询条件 (With<Camera2d>, Without<PlayerId>) 没有找到任何相机！");
            // 调试输出已禁用: println!("[错误] 可能的原因：");
            // 调试输出已禁用: println!("  1. 相机有 PlayerId 组件（不应该有）");
            // 调试输出已禁用: println!("  2. 相机没有 Camera2d 组件（不应该发生）");
            // 调试输出已禁用: println!("[错误] 请检查相机创建代码，确保网络模式下相机没有 PlayerId 组件");
        }
    }
}

/// 网络模式下的安全检查：确保本地玩家的视角与其角色保持一致
/// （防止由于网络延迟或其它系统提前退出导致画面没有及时切换）
/// 优化：只在角色切换时检查，而不是每帧检查
pub fn ensure_network_view_matches_role_system(
    mut view_config: ResMut<crate::ViewConfig>,
    mut last_role_state: ResMut<LastRoleState>,
    room_info: Option<Res<crate::RoomInfo>>,
    changed_player_query: Query<(&crate::PlayerId, &PlayerRole), Changed<PlayerRole>>,
    all_player_query: Query<(&crate::PlayerId, &PlayerRole)>,
    mut camera_switch_writer: EventWriter<CameraSwitchEvent>,
) {
    let Some(room_info) = room_info else {
        return;
    };

    if !room_info.is_connected {
        return;
    }

    let local_player_id = if room_info.is_host {
        crate::PlayerId::Player1
    } else {
        crate::PlayerId::Player2
    };

    // 优化：只在角色改变时检查，或者首次运行时检查
    let mut local_role: Option<PlayerRole> = None;
    let mut role_changed = false;
    
    // 首先检查是否有角色改变（使用Changed过滤器）
    for (player_id, role) in changed_player_query.iter() {
        if *player_id == local_player_id {
            local_role = Some(*role);
            // 如果角色与上次不同，标记为改变
            if last_role_state.last_local_role != local_role {
                role_changed = true;
            }
            break;
        }
    }
    
    // 如果没有检测到改变，查询所有玩家来获取当前角色（用于首次运行或初始化）
    if local_role.is_none() {
        for (player_id, role) in all_player_query.iter() {
            if *player_id == local_player_id {
                local_role = Some(*role);
                // 检查是否与缓存不同
                role_changed = last_role_state.last_local_role != local_role;
                break;
            }
        }
    }
    
    // 如果没有角色改变，且上次的状态与当前视图配置一致，则直接返回
    if !role_changed && last_role_state.last_local_role.is_some() {
        if last_role_state.last_view_config == view_config.is_attacker_view {
            return; // 没有改变，跳过检查
        }
    }

    let Some(local_role) = local_role else {
        return;
    };

    let should_be_attacker_view = matches!(local_role, PlayerRole::Attacker);
    
    // 只在角色改变或视图配置不匹配时才更新
    if role_changed || view_config.is_attacker_view != should_be_attacker_view {
        view_config.is_attacker_view = should_be_attacker_view;
        camera_switch_writer.send(CameraSwitchEvent {
            is_attacker_view: should_be_attacker_view,
        });
        // 不再直接修改相机，让 switch_network_camera_system 通过事件来处理
        // apply_network_camera_view(&mut commands, &mut camera_query, should_be_attacker_view);
        
        // 更新缓存
        last_role_state.last_local_role = Some(local_role);
        last_role_state.last_view_config = should_be_attacker_view;
    }
}

// --- 游玩系统实现 ---

/// 攻击方：瞄准（WASD控制）
pub fn attacker_aim_system(
    time: Res<Time>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut cursor_pos: ResMut<CursorPosition>,
    mut crosshair_offset: ResMut<CrosshairOffset>,
    mut camera_query: Query<(&mut Transform, &Camera, Option<&PlayerCamera>), (With<Camera2d>, Without<Crosshair>, Without<DefenderCamera>)>,
    player_query: Query<(&PlayerRole, &PlayerId), (With<PlayerId>, Without<DefenderCamera>)>,
    view_config: Res<ViewConfig>,
    room_info: Option<Res<crate::RoomInfo>>,
) {
    // 检查是否为本地模式
    let is_local_mode = room_info.as_ref().map(|r| !r.is_connected).unwrap_or(false);
    
    // 网络模式下，只允许当前是进攻方的玩家操控
    if let Some(room_info) = room_info.as_ref() {
        if room_info.is_connected {
            // 网络模式下，只有当前视图是进攻方的玩家才能操控
            if !view_config.is_attacker_view {
                return;
            }
        }
    }
    
    // 本地模式下，需要确定哪个玩家正在操控
    let controlling_player = if is_local_mode {
        // 在本地模式下，根据输入确定是P1（WASD）还是P2（方向键）
        // 但这里我们只处理WASD输入，所以是P1
        // 如果P1是进攻方，则允许操控
        player_query.iter()
            .find(|(_, player_id)| **player_id == PlayerId::Player1)
            .and_then(|(role, _)| if matches!(role, PlayerRole::Attacker) { Some(PlayerId::Player1) } else { None })
    } else {
        // 网络模式或单机模式
        if !view_config.is_attacker_view { return; }
        None // 网络模式下不需要检查玩家ID
    };
    
    // 本地模式下，如果P1不是进攻方，则不允许操控
    if is_local_mode {
        if controlling_player.is_none() {
            return; // P1不是进攻方，不允许操控
        }
    } else {
        if !view_config.is_attacker_view { return; }
    }
    
    let attacker_pos = ATTACKER_START_POS.truncate();
    let mut move_direction = Vec2::ZERO;
    if keyboard_input.pressed(KeyCode::KeyW) { move_direction.y += 1.0; }
    if keyboard_input.pressed(KeyCode::KeyS) { move_direction.y -= 1.0; }
    if keyboard_input.pressed(KeyCode::KeyA) { move_direction.x -= 1.0; }
    if keyboard_input.pressed(KeyCode::KeyD) { move_direction.x += 1.0; }
    
    if move_direction.length_squared() > 0.0 {
        move_direction = move_direction.normalize();
        let movement = move_direction * AIM_SPEED * time.delta_seconds();
        crosshair_offset.0 += movement;
        crosshair_offset.0 = crosshair_offset.0.clamp_length_max(MAX_AIM_OFFSET);
    }
    
    let aim_world_pos = attacker_pos + crosshair_offset.0;
    cursor_pos.0 = aim_world_pos;
    
    // 进攻方摄像机跟随瞄准点移动，保持准星在屏幕中心
    if is_local_mode {
        // 本地模式：只更新P1的相机（如果P1是进攻方）
        if let Some(controlling_player) = controlling_player {
            for (mut camera_transform, _, player_camera) in camera_query.iter_mut() {
                if let Some(player_camera) = player_camera {
                    if player_camera.player_id == controlling_player {
                        camera_transform.translation.x = aim_world_pos.x;
                        camera_transform.translation.y = aim_world_pos.y;
                        break;
                    }
                }
            }
        }
    } else {
        // 网络模式：更新进攻方相机
        // 进攻方相机跟随瞄准点移动，保持准星在屏幕中心
        // 注意：这里查询排除了DefenderCamera，所以只匹配进攻方相机
        if view_config.is_attacker_view {
            for (mut camera_transform, camera, _) in camera_query.iter_mut() {
                // 跳过UI相机（order > 0）
                if camera.order > 0 {
                    continue;
                }
                // 在网络模式下，这个查询只匹配进攻方相机（没有DefenderCamera组件，order = 0）
                // 相机位置 = 瞄准点位置，这样瞄准点（准星）就会在屏幕中心
                // 进攻方玩家位置固定（ATTACKER_START_POS），但相机跟随瞄准点移动
                // 注意：只有在进攻方视图时才更新相机位置，避免在角色切换时覆盖相机设置
                camera_transform.translation.x = aim_world_pos.x;
                camera_transform.translation.y = aim_world_pos.y;
                camera_transform.translation.z = 1000.0; // 保持Z轴为1000.0
                break; // 网络模式下只有一个游戏相机，更新后退出
            }
        } else {
            // 防守方视图：不更新相机位置（相机应该固定在墙的位置）
            // 这个检查确保在角色切换后，如果视图配置已经更新为防守方视图，
            // attacker_aim_system 不会覆盖防守方相机的设置
            return;
        }
    }
}

/// 攻击方射击系统（无残留轨迹）
/// 游戏结束延迟系统：延迟2秒后发送游戏结束事件和网络消息
pub fn game_over_delay_system(
    time: Res<Time>,
    mut game_over_delay: ResMut<GameOverDelay>,
    mut game_over_events: EventWriter<GameOverEvent>,
    mut next_app_state: ResMut<NextState<AppState>>,
    room_info: Option<Res<crate::RoomInfo>>,
    network_manager: Option<Res<crate::network_game::NetworkManager>>,
    player_query: Query<(&Transform, &PlayerId, &PlayerRole, &DodgeAction, &Health)>,
    round_info: Option<Res<RoundInfo>>,
) {
    let Some(mut timer) = game_over_delay.timer.as_mut() else {
        // 如果延迟计时器不存在，检查是否需要持续发送网络消息
        // 持续发送GameOver网络消息（3秒内，每0.1秒发送一次）
        if let Some(ref mut send_timer) = game_over_delay.network_send_timer {
            send_timer.tick(time.delta());
            
            if send_timer.just_finished() {
                game_over_delay.network_send_count += 1;
                
                // 3秒 = 30次（每0.1秒一次）
                if game_over_delay.network_send_count >= 30 {
                    // 停止发送
                    game_over_delay.network_send_timer = None;
                    game_over_delay.network_send_count = 0;
                    // 调试输出已禁用: println!("[游戏结束调试] 房主停止持续发送GameOver消息（已发送30次）");
                } else {
                    // 继续发送
                    if let Some(winner_id) = game_over_delay.winner_id {
                        if let Some(nm) = network_manager.as_ref() {
                            if nm.is_host {
                                let game_over_msg = crate::network_game::NetworkMessage::GameOver {
                                    winner: winner_id,
                                };
                                // 调试输出已禁用: println!("[游戏结束调试] 房主持续发送GameOver网络消息 ({}/30): winner={:?}", 
                                //                          game_over_delay.network_send_count, winner_id);
                                crate::network_game::send_network_message(&**nm, game_over_msg);
                            }
                        }
                    }
                }
            }
        }
        return;
    };
    
    timer.tick(time.delta());
    
    if timer.finished() {
                let winner_id = game_over_delay.winner_id.take().unwrap();
                let loser_id = game_over_delay.loser_id.take().unwrap();
                game_over_delay.timer = None;
                
                let is_host = network_manager.as_ref().map(|nm| nm.is_host).unwrap_or(false);
                // 调试输出已禁用: println!("[游戏结束调试] 延迟计时器完成，发送GameOverEvent (is_host={})", is_host);
                
                // 发送游戏结束事件
                game_over_events.send(GameOverEvent {
                    winner_id,
                    loser_id,
                });
                
                // 调试输出已禁用: println!("[游戏结束调试] 已发送GameOverEvent，切换到GameOver状态");
                
                // 切换到游戏结束状态
                next_app_state.set(AppState::GameOver);
                
                // 如果是网络模式且是主机，发送游戏结束消息，并启动持续发送机制
                let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
                if is_network_mode {
                    if let Some(nm) = network_manager.as_ref() {
                        if nm.is_host {
                            // 收集所有玩家的数据
                            let mut player_data = Vec::new();
                            for (transform, player_id, role, _dodge_action, health_comp) in player_query.iter() {
                                let pos = transform.translation;
                                player_data.push((
                                    *player_id,
                                    [pos.x, pos.y, pos.z],
                                    health_comp.0,
                                    *role,
                                ));
                            }
                            
                            // 先强制同步一次游戏状态（包含最新的血量）
                            if let Some(round_info) = round_info.as_ref() {
                                crate::network_game::force_sync_game_state(
                                    &**nm,
                                    &player_data,
                                    round_info,
                                );
                            }
                            
                            // 立即发送第一次游戏结束消息
                            let game_over_msg = crate::network_game::NetworkMessage::GameOver {
                                winner: winner_id,
                            };
                            // 调试输出已禁用: println!("[游戏结束调试] 房主发送GameOver网络消息（立即）: winner={:?}", winner_id);
                            crate::network_game::send_network_message(&**nm, game_over_msg);
                            
                            // 启动持续发送机制：3秒内，每0.1秒发送一次（共30次）
                            game_over_delay.network_send_timer = Some(Timer::from_seconds(0.1, TimerMode::Repeating));
                            game_over_delay.network_send_count = 0;
                            game_over_delay.winner_id = Some(winner_id); // 保留winner_id用于后续发送
                            game_over_delay.loser_id = Some(loser_id); // 保留loser_id
                            // 调试输出已禁用: println!("[游戏结束调试] 房主启动持续发送机制：3秒内每0.1秒发送一次GameOver消息");
                        }
                    }
                }
            }
}

pub fn attacker_shoot_system(
    mut commands: Commands,
    mut round_info: ResMut<RoundInfo>,
    mut bullet_id_counter: ResMut<BulletIdCounter>,
    cursor_pos: Res<CursorPosition>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    attacker_query: Query<(&Transform, &PlayerRole, &PlayerId)>,
    mut player_query: Query<(&Transform, &PlayerId, &PlayerRole, &DodgeAction, &mut Health), (With<PlayerId>, Without<Bullet>)>,
    mut events: EventWriter<PlayerHitEvent>,
    mut game_over_delay: ResMut<GameOverDelay>,
    view_config: Res<ViewConfig>,
    room_info: Option<Res<crate::RoomInfo>>,
    network_manager: Option<Res<crate::network_game::NetworkManager>>,
    time: Res<Time>,
    mut shoot_cooldown: Local<f32>, // 射击冷却时间
) {
    // 网络模式下，只允许当前是进攻方的玩家射击
    if let Some(room_info) = room_info.as_ref() {
        if room_info.is_connected {
            // 网络模式下，只有当前视图是进攻方的玩家才能射击
            if !view_config.is_attacker_view {
                return;
            }
        }
    }
    
    if !view_config.is_attacker_view { return; }
    if round_info.is_switching { return; }
    
    // 网络模式下，检查本地玩家是否是当前的进攻方
    let is_network_mode = room_info.as_ref().map(|r| r.is_connected).unwrap_or(false);
    if is_network_mode {
        let local_player_id = room_info.as_ref().map(|r| {
            if r.is_host {
                PlayerId::Player1
            } else {
                PlayerId::Player2
            }
        }).unwrap_or(PlayerId::Player1);
        
        // 如果本地玩家不是当前的进攻方，不允许射击
        if local_player_id != round_info.current_attacker {
            return;
        }
    }
    
    let mut attacker_pos = ATTACKER_START_POS.truncate();
    let mut attacker_id = round_info.current_attacker;
    
    // 查找进攻方角色（优先匹配 current_attacker）
    let mut found_attacker = false;
    for (transform, role, id) in attacker_query.iter() {
        if *id == round_info.current_attacker {
            // 如果找到了 current_attacker，使用它的位置
            attacker_pos = transform.translation.truncate();
            attacker_id = *id;
            found_attacker = true;
            
            // 如果是网络模式，验证角色是否正确
            if is_network_mode {
                if !matches!(role, PlayerRole::Attacker) {
                    // 调试输出已禁用: println!("[警告] 玩家 {:?} 应该是进攻方，但角色是 {:?}，等待角色更新", id, role);
                }
            }
            break;
        }
    }
    
    // 如果没有找到 current_attacker，尝试查找任何进攻方角色（兼容性处理）
    if !found_attacker {
        for (transform, role, id) in attacker_query.iter() {
            if matches!(role, PlayerRole::Attacker) {
                attacker_pos = transform.translation.truncate();
                attacker_id = *id;
                found_attacker = true;
                // 调试输出已禁用: println!("[警告] 未找到 current_attacker ({:?})，使用进攻方角色 {:?}", round_info.current_attacker, id);
                break;
            }
        }
    }
    
    // 如果仍然没有找到进攻方，使用默认值（不应该发生）
    if !found_attacker {
        // 调试输出已禁用: println!("[错误] 未找到进攻方角色，current_attacker = {:?}", round_info.current_attacker);
        return;
    }

    // 更新射击冷却
    if *shoot_cooldown > 0.0 {
        *shoot_cooldown -= time.delta_seconds();
    }
    
    // 检查射击条件：J键按下、有子弹、时间未到、冷却完成
    if keyboard_input.just_pressed(KeyCode::KeyJ) 
        && round_info.bullets_left > 0 
        && !round_info.round_timer.finished()
        && *shoot_cooldown <= 0.0 {
        
        // 设置射击冷却（1秒）
        *shoot_cooldown = 1.0;
        
        round_info.bullets_left -= 1;
        round_info.bullets_fired_this_round += 1;

        let target_pos = cursor_pos.0;
        let direction = (target_pos - attacker_pos).normalize_or_zero();
        let velocity = direction * BULLET_SPEED;

        // 优化：减少日志输出以提高性能
        // println!("=== 射击: 进攻方={:?}, 准心位置=({:.1}, {:.1}) ===", attacker_id, target_pos.x, target_pos.y);
        
        // 检查是否为网络模式
        let is_network_mode = room_info.as_ref().map(|r| r.is_connected).unwrap_or(false);
        
        // 在网络模式下，发送子弹同步消息
        if is_network_mode {
            if let Some(network_manager) = network_manager.as_ref() {
                let sync_id = bullet_id_counter.0;
                let bullet_msg = crate::network_game::NetworkMessage::BulletSpawn {
                    bullet_id: sync_id,
                    owner: attacker_id,
                    start_pos: [attacker_pos.x, attacker_pos.y],
                    target_pos: [target_pos.x, target_pos.y],
                    velocity: [velocity.x, velocity.y],
                };
                crate::network_game::send_network_message(&**network_manager, bullet_msg);
            }
        }
        
        // 在网络模式下，查找防守方（可能是对方玩家）
        // 防守方位置应该已经通过 handle_player_input_system 更新
        let mut found_defender = false;
        let mut game_over_info: Option<(PlayerId, PlayerId)> = None; // (winner, loser)
        for (defender_transform, defender_id, defender_role, dodge_action, mut health) in player_query.iter_mut() {
            if !matches!(defender_role, PlayerRole::Defender) {
                continue;
            }
            
            if *defender_id == attacker_id { 
                continue;
            }
            
            found_defender = true;
            let defender_pos = defender_transform.translation.truncate();
            // 优化：减少日志输出以提高性能
            // println!("  [碰撞检测] 检查防守方: {:?}, 位置=({:.1}, {:.1}), 血量={:.1}", 
            //          defender_id, defender_pos.x, defender_pos.y, health.0);
            
            let offset = target_pos - defender_pos;
            let distance = offset.length();
            
            let player_height = PLAYER_SIZE.y;
            let player_width = PLAYER_SIZE.x;
            let is_crouching = matches!(dodge_action, DodgeAction::Crouch);
            let current_height = if is_crouching { player_height * 0.7 } else { player_height };
            
            // 改进碰撞检测：使用更宽松的距离检测
            // 考虑到网络延迟，使用更大的碰撞检测范围（1.5倍）
            let hitbox_radius = (player_width * player_width + current_height * current_height).sqrt() / 2.0;
            let max_hit_distance = hitbox_radius * 1.5; // 增加50%的容错范围，应对网络延迟
            
            if distance > max_hit_distance {
                // 调试输出已禁用: println!("  -> 准心距离防守方太远: {:.1} > {:.1}（容错范围），未命中", distance, max_hit_distance);
                continue;
            }
            
            // 使用更宽松的相对位置检测（1.2倍容错）
            let relative_y = offset.y / (current_height / 2.0);
            let relative_x = offset.x / (player_width / 2.0);
            
            if relative_x.abs() > 1.2 || relative_y.abs() > 1.2 {
                // 调试输出已禁用: println!("  -> 准心不在玩家身体范围内: 相对位置=({:.2}, {:.2})，容错范围=±1.2", relative_x, relative_y);
                continue;
            }
            
            let hit_part = if relative_y > 0.3 {
                Some(HitboxType::Head)
            } else if relative_y > -0.2 {
                Some(HitboxType::Torso)
            } else {
                Some(HitboxType::Legs)
            };
            
            if let Some(hitbox_type) = hit_part {
                let part_name = match hitbox_type {
                    HitboxType::Head => "头部",
                    HitboxType::Torso => "躯干",
                    HitboxType::Legs => "腿部",
                };
                // 调试输出已禁用: println!("  -> 命中{}！相对位置=({:.2}, {:.2})", part_name, relative_x, relative_y);
                
                let damage = match hitbox_type {
                    HitboxType::Head => DAMAGE_HEAD,
                    HitboxType::Torso => DAMAGE_TORSO,
                    HitboxType::Legs => DAMAGE_LEGS,
                };
                
                let old_health = health.0;
                health.0 = (health.0 - damage).max(0.0);
                
                match *defender_id {
                    PlayerId::Player1 => round_info.p1_health = health.0,
                    PlayerId::Player2 => round_info.p2_health = health.0,
                }
                
                // 调试输出已禁用: println!("  *** 造成伤害: {} 点，防守方 {:?} 血量: {:.1} -> {:.1} ***", damage, defender_id, old_health, health.0);
                
                // 在网络模式下，发送血量更新消息
                if is_network_mode {
                    if let Some(network_manager) = network_manager.as_ref() {
                        let health_msg = crate::network_game::NetworkMessage::HealthUpdate {
                            player_id: *defender_id,
                            health: health.0,
                        };
                        crate::network_game::send_network_message(&**network_manager, health_msg);
                    }
                }
                
                events.send(PlayerHitEvent {
                    player_id: *defender_id,
                    damage,
                    hitbox_type,
                });
                
                round_info.bullets_hit_defender += 1;
                
                if health.0 <= 0.0 {
                    let winner_id = match *defender_id {
                        PlayerId::Player1 => PlayerId::Player2,
                        PlayerId::Player2 => PlayerId::Player1,
                    };
                    
                    // 调试输出已禁用: println!("  *** 游戏结束: 防守方 {:?} 被淘汰！获胜者: {:?} ***", defender_id, winner_id);
                    
                    // 记录游戏结束信息，在循环外部处理延迟（避免借用冲突）
                    game_over_info = Some((winner_id, *defender_id));
                    
                    // 不立即发送事件和切换状态，延迟系统会在2秒后处理
                    // game_over_events.send(GameOverEvent {
                    //     winner_id,
                    //     loser_id: *defender_id,
                    // });
                    // next_app_state.set(AppState::GameOver);
                    
                    break;
                }
            }
        }

        // 如果在网络模式下没有找到防守方，记录警告
        if is_network_mode && !found_defender {
            // 调试输出已禁用: println!("  [警告] 网络模式下未找到防守方！可能防守方位置未同步");
        }
        
        // 在循环结束后，如果需要启动游戏结束延迟（避免借用冲突）
        if let Some((winner_id, loser_id)) = game_over_info {
            // 启动延迟计时器（2秒）
            game_over_delay.timer = Some(Timer::from_seconds(2.0, TimerMode::Once));
            game_over_delay.winner_id = Some(winner_id);
            game_over_delay.loser_id = Some(loser_id);
            
            // 不立即发送网络消息，延迟系统会在2秒后发送
            // if is_network_mode {
            //     if let Some(nm) = network_manager.as_ref() {
            //         // ... 网络消息发送逻辑移到延迟系统 ...
            //     }
            // }
        }

        // 创建子弹（本地创建，网络模式下会通过BulletSpawn消息同步到对方）
        spawn_bullet(
            &mut commands,
            &mut bullet_id_counter,
            attacker_id,
            attacker_pos,
            target_pos,
            velocity,
        );
    }
}

/// 创建子弹（可被本地射击系统和网络同步系统调用）
pub fn spawn_bullet(
    commands: &mut Commands,
    bullet_id_counter: &mut BulletIdCounter,
    attacker_id: PlayerId,
    attacker_pos: Vec2,
    target_pos: Vec2,
    velocity: Vec2,
) {
    // 获取子弹同步ID
    let sync_id = bullet_id_counter.0;
    bullet_id_counter.0 += 1;
    
    spawn_bullet_with_id(commands, attacker_id, attacker_pos, target_pos, velocity, sync_id);
}

/// 从网络消息创建子弹（使用网络消息中的bullet_id）
pub fn spawn_bullet_with_id(
    commands: &mut Commands,
    attacker_id: PlayerId,
    attacker_pos: Vec2,
    target_pos: Vec2,
    velocity: Vec2,
    sync_id: u64,
) {
    // 枪口闪光：在两个渲染层都显示（进攻方和防守方都能看到）
    let muzzle_flash_pos = attacker_pos.extend(6.0);
    let muzzle_flash_color = Color::rgb(1.0, 0.8, 0.0);
    let muzzle_flash_size = Vec2::new(20.0, 20.0);
    
    // 进攻方视角的枪口闪光（RenderLayer 0）
        commands.spawn((
            MuzzleFlash { timer: Timer::from_seconds(MUZZLE_FLASH_DURATION, TimerMode::Once) },
            SpriteBundle {
            sprite: Sprite { color: muzzle_flash_color, custom_size: Some(muzzle_flash_size), ..default() },
            transform: Transform::from_translation(muzzle_flash_pos),
                ..default()
            },
        RenderLayers::layer(0), // 进攻方视角
        ));

    // 防守方视角的枪口闪光（RenderLayer 1）
    commands.spawn((
        MuzzleFlash { timer: Timer::from_seconds(MUZZLE_FLASH_DURATION, TimerMode::Once) },
        SpriteBundle {
            sprite: Sprite { color: muzzle_flash_color, custom_size: Some(muzzle_flash_size), ..default() },
            transform: Transform::from_translation(muzzle_flash_pos),
            ..default()
        },
        RenderLayers::layer(1), // 防守方视角
    ));

    // 子弹：在两个渲染层都显示（进攻方和防守方都能看到）
    let bullet_color = if attacker_id == PlayerId::Player1 { Color::rgb(1.0, 0.9, 0.0) } else { Color::rgb(0.0, 0.9, 1.0) };
    let bullet_pos = attacker_pos.extend(5.0);
    
    // 进攻方视角的子弹（RenderLayer 0）
        commands.spawn((
            Bullet { 
                velocity, 
                start_pos: attacker_pos, 
                target_pos,
                owner: attacker_id 
            },
        BulletSyncId(sync_id), // 使用网络消息中的同步ID
            SpriteBundle {
                sprite: Sprite {
                color: bullet_color,
                    custom_size: Some(BULLET_SIZE),
                    ..default()
                },
            transform: Transform::from_translation(bullet_pos),
                visibility: Visibility::Visible,
                ..default()
            },
            Collider { size: BULLET_SIZE },
        RenderLayers::layer(0), // 进攻方视角
        ));
    
    // 防守方视角的子弹（RenderLayer 1）
    commands.spawn((
        Bullet { 
            velocity, 
            start_pos: attacker_pos, 
            target_pos,
            owner: attacker_id 
        },
        BulletSyncId(sync_id), // 使用网络消息中的同步ID
        SpriteBundle {
            sprite: Sprite {
                color: bullet_color,
                custom_size: Some(BULLET_SIZE),
                ..default()
            },
            transform: Transform::from_translation(bullet_pos),
            visibility: Visibility::Visible,
            ..default()
        },
        Collider { size: BULLET_SIZE },
        RenderLayers::layer(1), // 防守方视角
    ));
}

/// 防守方：移动（方向键）
pub fn defender_move_system(
    time: Res<Time>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &PlayerRole, &Collider, &PlayerId)>,
    view_config: Res<crate::ViewConfig>,
    room_info: Option<Res<crate::RoomInfo>>,
    app_state: Res<State<crate::AppState>>,
) {
    // 如果游戏已结束，不允许移动
    if *app_state.get() == crate::AppState::GameOver {
        return;
    }
    
    // 检查是否为网络模式
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    
    // 网络模式下，只有当前视图是防守方的玩家才能操控防守方
    if is_network_mode && view_config.is_attacker_view {
        return; // 进攻方视角，不允许操控防守方
    }
    
    // 在网络模式下，确定本地玩家ID
    let local_player_id_opt = if is_network_mode {
        room_info.as_ref().map(|r| {
            if r.is_host {
                PlayerId::Player1
            } else {
                PlayerId::Player2
            }
        })
    } else {
        None // 本地模式下，不限制玩家ID
    };
    
    for (mut transform, role, collider, player_id) in query.iter_mut() {
        if let PlayerRole::Defender = role {
            // 网络模式下，只允许操控本地玩家的防守方角色
            if is_network_mode {
                if let Some(local_player_id) = local_player_id_opt {
                    if *player_id != local_player_id {
                        continue; // 只能操控本地玩家的角色
                    }
                } else {
                    continue; // 如果无法确定本地玩家ID，跳过
                }
            }
            
            // 收集移动输入（使用WASD）
            let mut move_direction = Vec2::ZERO;
            if keyboard_input.pressed(KeyCode::KeyW) { move_direction.y += 1.0; }
            if keyboard_input.pressed(KeyCode::KeyS) { move_direction.y -= 1.0; }
            if keyboard_input.pressed(KeyCode::KeyA) { move_direction.x -= 1.0; }
            if keyboard_input.pressed(KeyCode::KeyD) { move_direction.x += 1.0; }
            
            // 如果没有输入，跳过移动（但不跳过后续处理）
            if move_direction.length_squared() > 0.0 {
                move_direction = move_direction.normalize();
            
            // 防守方移动速度是原来的80%
            let defender_move_speed = PLAYER_MOVE_SPEED * 0.8;
            let movement = move_direction * defender_move_speed * time.delta_seconds();
                let old_x = transform.translation.x;
                let old_y = transform.translation.y;
                let new_x = old_x + movement.x;
                let new_y = old_y + movement.y;
            
                // 限制移动范围（在墙的范围内）
            let wall_half_width = WALL_SIZE.x / 2.0;
            let player_half_width = collider.size.x / 2.0;
            transform.translation.x = new_x.clamp(-wall_half_width + player_half_width, wall_half_width - player_half_width);
            
            let wall_half_height = WALL_SIZE.y / 2.0;
            let player_half_height = collider.size.y / 2.0;
            transform.translation.y = new_y.clamp(
                WALL_POSITION.y - wall_half_height + player_half_height,
                WALL_POSITION.y + wall_half_height - player_half_height
            );
                
                // 调试：确认玩家位置已更新
                if (transform.translation.x != old_x || transform.translation.y != old_y) && is_network_mode {
                    // 每60帧打印一次（约1秒）
                    if (time.elapsed_seconds() * 60.0) as u32 % 60 == 0 {
                        // 调试输出已禁用: println!("[防守方移动] 玩家 {:?} 位置: ({:.1}, {:.1}) -> ({:.1}, {:.1})", player_id, old_x, old_y, transform.translation.x, transform.translation.y);
                    }
                }
            }
        }
    }
}

/// 防守方：动作系统（下蹲、侧躲）
pub fn defender_action_system(
    time: Res<Time>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &PlayerRole, &mut ActionCooldown, &mut Collider, &mut DodgeAction, &PlayerId)>,
    mut events: EventWriter<PlayerActionEvent>,
    view_config: Res<crate::ViewConfig>,
    room_info: Option<Res<crate::RoomInfo>>,
    app_state: Res<State<crate::AppState>>,
) {
    // 如果游戏已结束，不允许动作
    if *app_state.get() == crate::AppState::GameOver {
        return;
    }
    
    // 检查是否为网络模式
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    
    // 网络模式下，只有当前视图是防守方的玩家才能操控防守方
    if is_network_mode && view_config.is_attacker_view {
        return; // 进攻方视角，不允许操控防守方
    }
    
    for (_transform, role, mut cooldown, _collider, mut dodge_action, player_id) in query.iter_mut() {
        if let PlayerRole::Defender = role {
            // 网络模式下，只允许操控本地玩家的防守方角色
            if is_network_mode {
                // 检查是否是本地玩家的角色
                if let Some(room_info) = room_info.as_ref() {
                    let local_player_id = if room_info.is_host {
                        PlayerId::Player1
                    } else {
                        PlayerId::Player2
                    };
                    if *player_id != local_player_id {
                        continue; // 只能操控本地玩家的角色
                    }
                }
            }
            let current_time = time.elapsed_seconds_f64();
            let time_since_last_action = current_time - cooldown.last_action_time;
            
            if time_since_last_action >= cooldown.cooldown_duration {
                // K键触发技能：随机选择下蹲或侧躲
                if keyboard_input.just_pressed(KeyCode::KeyK) {
                    let action = if rand::thread_rng().gen_bool(0.5) {
                        DodgeAction::Crouch
                    } else {
                        if rand::thread_rng().gen_bool(0.5) {
                            DodgeAction::SideLeft
                        } else {
                            DodgeAction::SideRight
                        }
                    };
                    *dodge_action = action;
                    events.send(PlayerActionEvent { player_id: *player_id, action });
                    cooldown.last_action_time = current_time;
                }
            }
        }
    }
}

/// 动作计时器系统
pub fn action_timer_system(
    time: Res<Time>,
    mut commands: Commands,
    mut param_set: ParamSet<(
        Query<(Entity, &mut ActionTimer, &mut Sprite)>,
        Query<(Entity, &mut Transform, &mut Collider, &mut DodgeAction, &PlayerId)>,
    )>,
) {
    let mut action_timer_query = param_set.p0();
    let mut finished_actions = Vec::new();
    for (entity, mut timer, mut sprite) in action_timer_query.iter_mut() {
        timer.timer.tick(time.delta());
        match timer.action {
            DodgeAction::Crouch => {
                sprite.color.set_a(0.8);
            }
            DodgeAction::SideLeft | DodgeAction::SideRight => {
                let progress = timer.timer.elapsed_secs() / timer.timer.duration().as_secs_f32();
                sprite.color.set_a(if (progress * 10.0) % 2.0 < 1.0 { 0.5 } else { 1.0 });
            }
            _ => {}
        }
        if timer.timer.finished() {
            finished_actions.push((timer.player_id, timer.action));
            commands.entity(entity).despawn();
        }
    }
    
    let mut player_query = param_set.p1();
    let mut new_actions = Vec::new();
    for (entity, mut transform, _collider, mut dodge_action, player_id) in player_query.iter_mut() {
        for (finished_player_id, finished_action) in &finished_actions {
            if *finished_player_id == *player_id && *dodge_action == *finished_action {
                *dodge_action = DodgeAction::None;
            }
        }
        
        if *dodge_action != DodgeAction::None {
            let transform_pos = transform.translation;
            new_actions.push((*player_id, *dodge_action, transform_pos, entity));
            
            match *dodge_action {
                DodgeAction::Crouch => {}
                DodgeAction::SideLeft => transform.translation.x -= SIDE_DODGE_DISTANCE,
                DodgeAction::SideRight => transform.translation.x += SIDE_DODGE_DISTANCE,
                _ => {}
            }
        }
    }
    
    for (player_id, action, transform_pos, _entity) in new_actions {
        let effect_color = match player_id {
            PlayerId::Player1 => Color::rgb(0.2, 0.4, 1.0),
            PlayerId::Player2 => Color::rgb(0.2, 1.0, 0.4),
        };
        
        commands.spawn((
            ActionTimer { 
                timer: Timer::from_seconds(ACTION_DURATION_SECONDS, TimerMode::Once), 
                action,
                player_id,
            },
            SpriteBundle {
                sprite: Sprite { color: effect_color, custom_size: Some(PLAYER_SIZE), ..default() },
                transform: Transform::from_translation(transform_pos),
                ..default()
            },
        ));
    }
}

/// 更新准星位置
/// 准星固定在屏幕中心（相对于相机的位置为 (0, 0)）
/// 进攻方相机跟随瞄准点移动，但准星保持在屏幕中心
/// 由于准星的初始位置是 (0, 0, 302.0)，而相机跟随瞄准点移动
/// 所以准星会始终在屏幕中心，只需要将准星的世界坐标设置为相机位置
pub fn update_crosshair_position_system(
    mut crosshair_query: Query<&mut Transform, With<Crosshair>>,
    cursor_pos: Res<CursorPosition>,
    view_config: Res<ViewConfig>,
    camera_query: Query<(&Transform, Option<&RenderLayers>, Option<&DefenderCamera>, &Camera), (With<Camera2d>, Without<Crosshair>)>,
) {
    if !view_config.is_attacker_view { return; }
    
    // 找到进攻方相机的位置（RenderLayer 0 且不是防守方相机，order=0）
    let mut camera_pos_opt = None;
    for (transform, layers_opt, defender_camera_opt, camera) in camera_query.iter() {
        if camera.order > 0 {
            continue; // 跳过UI相机
        }
        if defender_camera_opt.is_some() {
            continue; // 跳过防守方相机
        }
        // 如果存在 RenderLayers，则需包含 layer 0
        if let Some(layers) = layers_opt {
            if !layers.intersects(&RenderLayers::layer(0)) {
                continue;
            }
        }
        camera_pos_opt = Some(transform.translation.truncate());
        break;
    }
    
    // 如果找不到相机，使用瞄准点位置（cursor_pos 就是瞄准点位置）
    let camera_pos = camera_pos_opt.unwrap_or(cursor_pos.0);
    
    for mut crosshair_transform in crosshair_query.iter_mut() {
        // 准星固定在屏幕中心（相对于相机的位置为 (0, 0)）
        // 由于相机跟随瞄准点移动，准星会始终在屏幕中心
        // 准星的世界坐标 = 相机位置（因为准星相对于相机在 (0, 0)）
        crosshair_transform.translation.x = camera_pos.x;
        crosshair_transform.translation.y = camera_pos.y;
        // Z轴保持302.0或303.0（准星在相机前方）
    }
}

/// 更新防守方视角的准星指示器位置
pub fn update_defender_crosshair_indicator_system(
    mut indicator_query: Query<&mut Transform, With<DefenderCrosshairIndicator>>,
    cursor_pos: Res<CursorPosition>,
) {
    let aim_pos = cursor_pos.0;
    for mut indicator_transform in indicator_query.iter_mut() {
        indicator_transform.translation.x = aim_pos.x;
        indicator_transform.translation.y = aim_pos.y;
    }
}

/// 更新人形sprite位置和大小
pub fn update_humanoid_sprite_positions(
    mut player_query: Query<(&Transform, &PlayerId, &DodgeAction, &mut Collider), (With<PlayerId>, Without<HumanoidPart>)>,
    mut humanoid_query: Query<(&mut Transform, &mut Sprite, &HumanoidPart), (With<HumanoidPart>, Without<PlayerId>)>,
) {
    for (player_transform, player_id, dodge_action, mut collider) in player_query.iter_mut() {
        let player_pos = player_transform.translation;
        let player_height = PLAYER_SIZE.y;
        let player_width = PLAYER_SIZE.x;
        
        let is_crouching = matches!(dodge_action, DodgeAction::Crouch);
        let current_height = if is_crouching { player_height * 0.7 } else { player_height };
        
        collider.size = Vec2::new(player_width, current_height);
        
        let head_height = current_height * 0.3;
        let torso_height = current_height * 0.5;
        let legs_height = current_height * 0.2;
        
        let head_y = player_pos.y + (current_height - head_height) / 2.0;
        let torso_y = player_pos.y - (current_height - torso_height) / 2.0 + head_height / 2.0;
        let legs_y = player_pos.y - (current_height - legs_height) / 2.0;
        
        for (mut part_transform, mut part_sprite, humanoid_part) in humanoid_query.iter_mut() {
            if humanoid_part.player_id == *player_id {
                let z_pos = match humanoid_part.view_layer {
                    ViewLayer::AttackerView => 1.0,
                    ViewLayer::DefenderView => 2.0,
                };
                
                match humanoid_part.part_type {
                    HumanoidPartType::Head => {
                        part_transform.translation.x = player_pos.x;
                        part_transform.translation.y = head_y;
                        part_transform.translation.z = z_pos;
                        part_sprite.custom_size = Some(Vec2::new(player_width * 0.8, head_height));
                    }
                    HumanoidPartType::Torso => {
                        part_transform.translation.x = player_pos.x;
                        part_transform.translation.y = torso_y;
                        part_transform.translation.z = z_pos;
                        part_sprite.custom_size = Some(Vec2::new(player_width * 0.9, torso_height));
                    }
                    HumanoidPartType::Legs => {
                        part_transform.translation.x = player_pos.x;
                        part_transform.translation.y = legs_y;
                        part_transform.translation.z = z_pos;
                        part_sprite.custom_size = Some(Vec2::new(player_width * 0.7, legs_height));
                    }
                }
            }
        }
    }
}

/// 子弹移动和轨迹更新
/// 由于每个子弹有两个副本（RenderLayer 0和1），我们需要同步它们的位置
pub fn bullet_movement_system(
    mut commands: Commands,
    time: Res<Time>,
    mut bullet_query: Query<(Entity, &Bullet, &mut Transform, &BulletSyncId), With<Bullet>>,
    q_window: Query<&Window, With<PrimaryWindow>>,
) {
    let window = q_window.single();
    let half_width = window.width() / 2.0;
    let half_height = window.height() / 2.0;

    // 按同步ID分组，确保同一颗子弹的两个副本位置同步
    let mut bullet_positions: HashMap<BulletSyncId, Vec3> = HashMap::new();
    let mut to_despawn: Vec<Entity> = Vec::new();
    let mut despawned_ids: Vec<BulletSyncId> = Vec::new();
    
    // 第一遍：计算新位置并检查边界
    for (entity, bullet, mut transform, sync_id) in bullet_query.iter_mut() {
        transform.translation.x += bullet.velocity.x * time.delta_seconds();
        transform.translation.y += bullet.velocity.y * time.delta_seconds();

        // 检查是否出界
        if transform.translation.x.abs() > half_width || transform.translation.y.abs() > half_height {
            to_despawn.push(entity);
            despawned_ids.push(*sync_id);
            continue;
        }
        
        // 存储新位置（如果已经有其他副本，使用第一个的位置）
        if !bullet_positions.contains_key(sync_id) {
            bullet_positions.insert(*sync_id, transform.translation);
        }
    }
    
    // 删除出界的子弹
    for entity in to_despawn {
            commands.entity(entity).despawn();
    }
    
    // 第二遍：同步所有相同sync_id的子弹位置
    for (_entity, _bullet, mut transform, sync_id) in bullet_query.iter_mut() {
        if let Some(sync_pos) = bullet_positions.get(sync_id) {
            transform.translation = *sync_pos;
        }
    }
}

/// 枪口闪光更新
pub fn muzzle_flash_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut MuzzleFlash, &mut Sprite)>,
) {
    for (entity, mut flash, mut sprite) in query.iter_mut() {
        flash.timer.tick(time.delta());
        sprite.color.set_a(1.0 - flash.timer.elapsed_secs() / flash.timer.duration().as_secs_f32());
        if flash.timer.finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// 碰撞检测系统
pub fn collision_detection_system(
    mut commands: Commands,
    bullet_query: Query<(Entity, &Transform, &Collider, &Bullet)>,
    mut wall_segment_query: Query<(Entity, &mut WallSegment, &mut Sprite, &mut Visibility, &Transform, &Collider), With<WallSegment>>,
    mut wall_query: Query<&mut Wall>,
    mut broken_wall_data: Option<ResMut<crate::BrokenWallData>>,
    room_info: Option<Res<crate::RoomInfo>>,
) {
    for (bullet_entity, bullet_transform, _bullet_collider, bullet) in bullet_query.iter() {
        let bullet_pos = bullet_transform.translation;
        let mut bullet_hit_something = false;
        let mut bullet_can_pass = false;

        let bullet_start = bullet.start_pos;
        let bullet_target = bullet.target_pos;
        
        let mut hit_wall_segment_pos = None;
        for (_segment_entity, segment, _sprite, _visibility, segment_transform, segment_collider) in wall_segment_query.iter_mut() {
            if segment.damaged { continue; }
            
            let segment_pos = segment_transform.translation.truncate();
            
            let is_ray_hit_wall = check_line_collision(
                bullet_start, 
                bullet_target,
                segment_pos,
                segment_collider.size,
            );
            
            let distance_to_target = (segment_pos - bullet_target).length();
            let is_near_target = distance_to_target < CROSSHAIR_DAMAGE_RANGE;

            if is_ray_hit_wall && is_near_target && hit_wall_segment_pos.is_none() {
                hit_wall_segment_pos = Some(segment_pos);
            }
        }

        if let Some(hit_pos) = hit_wall_segment_pos {
            let brick_width = BRICK_WIDTH;
            let damage_range = brick_width * 1.5;
            
            let mut segments_to_damage = Vec::new();
            for (_segment_entity, segment, _sprite, _visibility, segment_transform, _) in wall_segment_query.iter() {
                if segment.damaged { continue; }
                
                let segment_pos = segment_transform.translation.truncate();
                let distance_to_hit = (segment_pos - hit_pos).length();
                if distance_to_hit < damage_range {
                    segments_to_damage.push(segment_pos);
                    if segments_to_damage.len() >= 3 {
                        break;
                    }
                }
            }

            for hit_segment_pos in segments_to_damage {
                for (_segment_entity, mut segment, mut sprite, mut visibility, segment_transform, _) in wall_segment_query.iter_mut() {
                    if segment.damaged { continue; }
                    
                    let segment_pos = segment_transform.translation.truncate();
                    if (segment_pos - hit_segment_pos).length() < 1.0 {
                        segment.damaged = true;
                        
                        // 记录破碎墙体位置（主机和客户端分别存储）
                        if let (Some(mut broken_data), Some(room)) = (broken_wall_data.as_mut(), room_info.as_ref()) {
                            if room.is_host {
                                // 主机：根据当前视角存储
                                if !broken_data.host_broken_segments.contains(&segment_pos) {
                                    broken_data.host_broken_segments.push(segment_pos);
                                    // 调试输出已禁用: println!("[破碎墙体] 主机记录破碎墙体位置: {:?} (视角: {:?})", segment_pos, segment.view_layer);
                                }
                            } else {
                                // 客户端：根据当前视角存储
                                if !broken_data.client_broken_segments.contains(&segment_pos) {
                                    broken_data.client_broken_segments.push(segment_pos);
                                    // 调试输出已禁用: println!("[破碎墙体] 客户端记录破碎墙体位置: {:?} (视角: {:?})", segment_pos, segment.view_layer);
                                }
                            }
                        }
                        
                        match segment.view_layer {
                            ViewLayer::AttackerView => {
                                sprite.color = Color::rgba(0.0, 0.0, 0.0, 0.0);
                                *visibility = Visibility::Hidden;
                            }
                            ViewLayer::DefenderView => {
                                sprite.color = Color::rgba(0.0, 0.0, 0.0, 0.8);
                                *visibility = Visibility::Visible;
                                
                                commands.spawn((
                                    SpriteBundle {
                                        sprite: Sprite { color: Color::rgba(0.0, 0.0, 0.0, 0.9), custom_size: Some(Vec2::new(30.0, 30.0)), ..default() },
                                        transform: Transform::from_translation(segment_transform.translation + Vec3::new(0.0, 0.0, 0.5)),
                                        ..default()
                                    },
                                    RenderLayers::layer(1),
                                ));
                            }
                        }
                        
                        bullet_can_pass = true;
                        bullet_hit_something = true;
                    }
                }
            }

            if let Ok(mut wall) = wall_query.get_single_mut() {
                wall.damaged = true;
                wall.damage_positions = hit_pos;
            }
        }

        if bullet_hit_something && !bullet_can_pass {
            commands.entity(bullet_entity).despawn();
            continue;
        }
        
        let bullet_distance = (bullet_pos.truncate() - bullet.start_pos).length();
        if bullet_distance > 1500.0 {
            commands.entity(bullet_entity).despawn();
        }
    }

    // 注意：角色切换逻辑由 delayed_round_switch_system 统一处理
    // 这里不再处理角色切换，避免重复逻辑
}

/// 网络模式：切换相机类型（角色切换时）
/// 优化：只在收到事件时运行，避免每帧检查
pub fn switch_network_camera_system(
    mut commands: Commands,
    mut camera_queries: ParamSet<(
        Query<(Entity, &Camera, &Transform, &Projection, Option<&DefenderCamera>, Option<&RenderLayers>), (With<Camera2d>, Without<PlayerCamera>, Without<IsDefaultUiCamera>)>,
        Query<(Entity, &mut Camera, &mut Transform, &mut Projection, Option<&DefenderCamera>, Option<&mut RenderLayers>), (With<Camera2d>, Without<PlayerCamera>, Without<IsDefaultUiCamera>)>,
    )>,
    view_config: Res<crate::ViewConfig>,
    room_info: Option<Res<crate::RoomInfo>>,
    mut switch_events: EventReader<CameraSwitchEvent>,
) {
    // 只在网络模式下执行
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if !is_network_mode {
        return;
    }
    
    // 优化：只在有事件时才处理，避免每帧检查
    let mut forced_view: Option<bool> = None;
    for event in switch_events.read() {
        forced_view = Some(event.is_attacker_view);
    }
    
    // 如果没有事件，直接返回（不进行每帧检查）
    let Some(desired_attacker_view) = forced_view else {
        return;
    };

    // 有事件时，直接应用视角切换
    // 使用 ParamSet 的可变查询来修改相机
    let mut camera_query = camera_queries.p1();
    
    // 直接修改相机（只处理游戏相机，order=0）
    for (entity, mut camera, mut transform, mut projection, defender_camera, mut render_layers) in camera_query.iter_mut() {
        // 只处理游戏相机（order=0），跳过UI相机
        if camera.order != 0 {
            continue;
        }
        
        if desired_attacker_view {
            // 进攻方视图：激活非防守方相机，停用防守方相机
            if defender_camera.is_none() {
                camera.is_active = true;
                if let Projection::Orthographic(ref mut ortho) = *projection {
                    ortho.scale = 0.5;
                }
                if let Some(ref mut rl) = render_layers {
                    **rl = RenderLayers::layer(0);
                } else {
                    commands.entity(entity).insert(RenderLayers::layer(0));
                }
            } else {
                camera.is_active = false;
            }
        } else {
            // 防守方视图：激活防守方相机，停用非防守方相机
            if defender_camera.is_some() {
                camera.is_active = true;
                if let Projection::Orthographic(ref mut ortho) = *projection {
                    ortho.scale = 1.5;
                }
                if let Some(ref mut rl) = render_layers {
                    **rl = RenderLayers::layer(1);
                } else {
                    commands.entity(entity).insert(RenderLayers::layer(1));
                }
            } else {
                camera.is_active = false;
            }
        }
    }
}

/// 摄像头跟随防守方系统
/// 防守方相机跟随防守方玩家移动（不是跟随墙！）
pub fn follow_defender_camera_system(
    mut camera_query: Query<(&mut Transform, &DefenderCamera, Option<&PlayerCamera>), Without<PlayerId>>,
    player_query: Query<(&Transform, &PlayerRole, &PlayerId), (With<PlayerId>, Without<DefenderCamera>)>,
    view_config: Res<crate::ViewConfig>,
    room_info: Option<Res<crate::RoomInfo>>,
) {
    // 网络模式下，只有当前视图是防守方视图时才更新相机
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    if is_network_mode {
        // 网络模式下，防守方相机固定在墙的位置，不跟随玩家
        // 只有在防守方视图时才更新（确保相机位置正确）
        if view_config.is_attacker_view {
            return; // 进攻方视角，不更新防守方相机
        }
        // 防守方视图：确保相机固定在墙的位置（即使角色刚切换）
        if let Some(room_info) = room_info.as_ref() {
            if room_info.is_connected {
                for (mut camera_transform, _, _) in camera_query.iter_mut() {
                    // 强制固定在墙的位置（确保角色切换后立即生效）
                    camera_transform.translation.x = crate::WALL_POSITION.x;
                    camera_transform.translation.y = crate::WALL_POSITION.y;
                    camera_transform.translation.z = 1000.0;
                }
            }
        }
        return; // 网络模式下防守方相机不跟随玩家，直接返回
    }
    
    // 本地模式：每个相机跟随对应的玩家（如果该玩家是防守方）
    let is_local_mode = room_info.as_ref().map(|r| !r.is_connected).unwrap_or(false);
    if is_local_mode {
        // 本地模式：每个相机跟随对应的玩家（如果该玩家是防守方）
        for (mut camera_transform, _, player_camera) in camera_query.iter_mut() {
            if let Some(player_camera) = player_camera {
                // 找到对应的玩家
                if let Some((player_transform, player_role, _player_id)) = player_query.iter()
                    .find(|(_, _, player_id)| **player_id == player_camera.player_id) {
                    // 只有当该玩家是防守方时才跟随
                    if matches!(player_role, PlayerRole::Defender) {
                        camera_transform.translation.x = player_transform.translation.x;
                        camera_transform.translation.y = player_transform.translation.y;
                        camera_transform.translation.z = 1000.0; // 保持Z轴为1000.0
                    }
                }
            }
        }
    }
}

/// 回合时间更新
pub fn round_timer_update_system(
    time: Res<Time>,
    mut round_info: ResMut<RoundInfo>,
    bullet_query: Query<Entity, With<Bullet>>,
    mut timer_text_query: Query<&mut Text, With<TimerText>>,
    font_resource: Res<crate::FontResource>,
) {
    let font = font_resource.font.clone();
    round_info.round_timer.tick(time.delta());
    
    // 更新所有TimerText（包括进攻方和防守方的）
    for mut text in timer_text_query.iter_mut() {
        // 更新字体
        if text.sections.len() >= 2 {
            text.sections[0].style.font = font.clone();
            text.sections[1].style.font = font.clone();
            text.sections[1].value = format!("{:.1}", round_info.round_timer.remaining_secs());
        }
    }
    
    // 检查时间是否到了（30秒）
    // 如果时间到了，将进攻方的子弹归0
    // 注意：不在这里设置 is_switching，让 delayed_round_switch_system 统一处理切换逻辑
    if round_info.round_timer.finished() {
        // 时间到0时，将进攻方的子弹归0
        if round_info.bullets_left > 0 {
            // 调试输出已禁用: println!("[回合时间] 30秒到了，将进攻方子弹归0（剩余: {}）", round_info.bullets_left);
            round_info.bullets_left = 0;
        }
    }
}

/// 胜利条件检测
pub fn check_win_condition_system(
    _query: Query<(&PlayerId, &Health)>,
) {
    // 游戏结束检查现在在 attacker_shoot_system 中完成
}

/// 延迟回合切换系统
/// 检查是否需要切换角色：三发子弹打完或30秒到了
/// 切换条件：所有子弹都消失了，且（三发子弹打完 或 30秒到了）
pub fn delayed_round_switch_system(
    mut round_info: ResMut<RoundInfo>,
    mut next_round_state: ResMut<NextState<RoundState>>,
    bullet_query: Query<Entity, With<Bullet>>,
    room_info: Option<Res<crate::RoomInfo>>,
    network_manager: Option<Res<crate::network_game::NetworkManager>>,
) {
    // 检查是否需要切换角色的条件：
    // 1. 所有子弹都消失了（bullet_query.is_empty()）
    // 2. 且（三发子弹打完 或 30秒到了）
    // 注意：不再检查是否命中防守方，只要子弹打完或时间到了就切换
    let bullets_finished = bullet_query.is_empty();
    let bullets_exhausted = round_info.bullets_left == 0;
    let time_expired = round_info.round_timer.finished();
    
    // 如果时间到了，即使还有子弹在飞行，也应该等待子弹消失后切换
    // 但如果时间到了且子弹已经归0，即使还有子弹在飞行，也应该标记为需要切换
    let should_switch = if time_expired {
        // 时间到了：只要子弹都消失了就切换（子弹已经归0）
        bullets_finished
    } else {
        // 时间未到：需要子弹用完且所有子弹都消失
        bullets_finished && bullets_exhausted
    };
    
    if should_switch && !round_info.is_switching {
            // 开始切换流程
            round_info.is_switching = true;
        // 调试输出已禁用: println!("[角色切换] 触发条件满足：子弹用完={}, 时间到期={}, 子弹剩余={}, 子弹消失={}", bullets_exhausted, time_expired, round_info.bullets_left, bullets_finished);
        
        // 网络模式下，主机发送角色切换消息
        if let (Some(room_info), Some(network_manager)) = (room_info.as_ref(), network_manager.as_ref()) {
            if room_info.is_connected && network_manager.is_host {
                // 计算新的进攻方：Player1 和 Player2 互换
                let new_attacker = match round_info.current_attacker {
                    PlayerId::Player1 => PlayerId::Player2,
                    PlayerId::Player2 => PlayerId::Player1,
                };
                
                // 更新当前进攻方（主机先更新，然后发送消息）
                round_info.current_attacker = new_attacker;
                
                // 发送角色切换消息给客户端
                let switch_msg = crate::network_game::NetworkMessage::SwitchRoles {
                    new_attacker,
                };
                crate::network_game::send_network_message(network_manager.as_ref(), switch_msg);
                // 调试输出已禁用: println!("[主机] 发送角色切换消息，新的进攻方: {:?} (房主变成防守方，客户端变成进攻方)", new_attacker);
                
                // 主机立即切换到切换状态
                next_round_state.set(RoundState::Switching);
                return;
            }
        }
        
        // 本地模式：直接切换
        if room_info.as_ref().map(|r| !r.is_connected).unwrap_or(true) {
            // 调试输出已禁用: println!("[本地模式] 所有子弹消失，开始切换角色");
            next_round_state.set(RoundState::Switching);
        } else {
            // 调试输出已禁用: println!("[网络模式] 等待主机发送角色切换消息...");
        }
    }
    
    // 如果已经在切换状态，且所有子弹都消失了，触发切换
    // 注意：这个分支主要用于处理之前已经设置了 is_switching 的情况（比如子弹打完时设置的）
    if round_info.is_switching && bullets_finished {
        // 如果是主机且网络模式，应该发送消息（如果之前没有发送）
        if let (Some(room_info), Some(network_manager)) = (room_info.as_ref(), network_manager.as_ref()) {
            if room_info.is_connected && network_manager.is_host {
                // 主机：发送角色切换消息（如果还没有发送）
                let new_attacker = match round_info.current_attacker {
                    PlayerId::Player1 => PlayerId::Player2,
                    PlayerId::Player2 => PlayerId::Player1,
                };
                round_info.current_attacker = new_attacker;
                let switch_msg = crate::network_game::NetworkMessage::SwitchRoles {
                    new_attacker,
                };
                crate::network_game::send_network_message(network_manager.as_ref(), switch_msg);
                // 调试输出已禁用: println!("[主机] 发送角色切换消息（is_switching=true分支），新的进攻方: {:?}", new_attacker);
                next_round_state.set(RoundState::Switching);
                round_info.is_switching = false;
                return;
            }
        }
        
        // 客户端等待主机消息，本地模式直接切换
        if room_info.as_ref().map(|r| !r.is_connected).unwrap_or(true) {
            // 调试输出已禁用: println!("[本地模式] 所有子弹消失，切换角色（is_switching=true）");
            next_round_state.set(RoundState::Switching);
            round_info.is_switching = false;
        } else {
            // 调试输出已禁用: println!("[网络模式] 已在切换状态，等待主机消息...");
        }
        // 客户端会在收到SwitchRoles消息后，由handle_client_role_switch触发切换
    } else if round_info.is_switching && !bullets_finished {
        // 调试输出已禁用: println!("[角色切换] 等待子弹消失... (is_switching=true, bullets_finished=false)");
    }
}

/// 攻防轮换
/// 网络模式下：Player1（房主）和 Player2（客户端）角色互换
/// 本地模式下：两个玩家角色互换
pub fn switch_roles_system(
    mut commands: Commands,
    mut query: Query<(Entity, &mut PlayerRole, &PlayerId, &mut Transform, &mut Collider, &mut DodgeAction)>,
    mut camera_query: Query<(Entity, &mut Camera, &mut Transform, &mut Projection, Option<&DefenderCamera>, Option<&mut RenderLayers>), (With<Camera2d>, Without<PlayerId>)>,
    all_cameras_query: Query<Entity, With<Camera2d>>, // 用于调试：查询所有相机实体
    camera_with_player_id: Query<Entity, (With<Camera2d>, With<PlayerId>)>, // 检查是否有相机有PlayerId
    camera_with_player_camera: Query<Entity, (With<Camera2d>, With<PlayerCamera>)>, // 检查是否有相机有PlayerCamera
    mut round_info: ResMut<RoundInfo>,
    mut next_round_state: ResMut<NextState<RoundState>>,
    mut view_config: ResMut<crate::ViewConfig>,
    mut camera_state_cache: ResMut<CameraStateCache>,
    room_info: Option<Res<crate::RoomInfo>>,
    mut camera_switch_writer: EventWriter<CameraSwitchEvent>,
) {
    // 调试输出已禁用: println!("[角色切换] ========== 开始切换角色 ==========");
    // 调试输出已禁用: println!("[角色切换] 当前 round_info.current_attacker = {:?}", round_info.current_attacker);
    
    // 网络模式下，需要更新视图配置
    let is_network_mode = room_info.is_some() && room_info.as_ref().unwrap().is_connected;
    let is_host = room_info.as_ref().map(|r| r.is_host).unwrap_or(false);
    
    // 调试输出已禁用: println!("[角色切换] 网络模式: {}, 是房主: {}", is_network_mode, is_host);
    
    // 调试：检查所有相机实体（不访问Camera组件，避免查询冲突）
    let all_cameras: Vec<Entity> = all_cameras_query.iter().collect();
    // 调试输出已禁用: println!("[角色切换] 调试：所有 Camera2d 相机实体数量: {}", all_cameras.len());
    for entity in &all_cameras {
        // 调试输出已禁用: println!("  - 相机 entity: {:?}", entity);
    }
    
    // 调试：检查是否有相机有PlayerId或PlayerCamera组件
    let cameras_with_player_id: Vec<Entity> = camera_with_player_id.iter().collect();
    let cameras_with_player_camera: Vec<Entity> = camera_with_player_camera.iter().collect();
    // 调试输出已禁用: println!("[角色切换] 调试：检查相机组件 - 有PlayerId的相机: {:?}, 有PlayerCamera的相机: {:?}", cameras_with_player_id, cameras_with_player_camera);
    if !cameras_with_player_id.is_empty() {
        // 调试输出已禁用: println!("[错误] 发现 {} 个相机有 PlayerId 组件（不应该有）: {:?}", cameras_with_player_id.len(), cameras_with_player_id);
        // 如果相机有PlayerId组件，尝试移除它
        for entity in &cameras_with_player_id {
            commands.entity(*entity).remove::<PlayerId>();
            // 调试输出已禁用: println!("[角色切换] 已移除相机 {:?} 的 PlayerId 组件", entity);
        }
    }
    if !cameras_with_player_camera.is_empty() {
        // 调试输出已禁用: println!("[错误] 发现 {} 个相机有 PlayerCamera 组件（网络模式不应该有）: {:?}", cameras_with_player_camera.len(), cameras_with_player_camera);
        // 如果相机有PlayerCamera组件，尝试移除它
        for entity in &cameras_with_player_camera {
            commands.entity(*entity).remove::<PlayerCamera>();
            // 调试输出已禁用: println!("[角色切换] 已移除相机 {:?} 的 PlayerCamera 组件", entity);
        }
    }
    
    // 调试：检查相机查询条件，看看为什么找不到相机
    if is_network_mode {
        // 调试输出已禁用: println!("[角色切换] 调试：网络模式下，应该找到1个游戏相机（order=0）和1个UI相机（order=1）");
        // 调试输出已禁用: println!("[角色切换] 调试：查询条件要求：With<Camera2d>, Without<PlayerId>");
        // 调试输出已禁用: println!("[角色切换] 调试：如果相机有PlayerId组件，将无法被查询到");
    }

    // 确定新的进攻方
    let new_attacker_id = if is_network_mode {
        // 网络模式：使用round_info.current_attacker（已经从网络消息中获取）
        // Player1 和 Player2 角色互换
        let new_attacker = round_info.current_attacker;
        
        // 调试输出已禁用: println!("[网络模式] 角色互换：新的进攻方 = {:?}", new_attacker);
        // 调试输出已禁用: println!("  - Player1 (房主): {} -> {}", if new_attacker == PlayerId::Player1 { "进攻方" } else { "防守方" }, if new_attacker == PlayerId::Player1 { "防守方" } else { "进攻方" });
        // 调试输出已禁用: println!("  - Player2 (客户端): {} -> {}", if new_attacker == PlayerId::Player2 { "进攻方" } else { "防守方" }, if new_attacker == PlayerId::Player2 { "防守方" } else { "进攻方" });
        
        // 根据新的进攻方更新角色
        for (entity, mut role, id, mut transform, mut collider, mut dodge_action) in query.iter_mut() {
            if *id == new_attacker {
                // 这个玩家是新的进攻方
                *role = PlayerRole::Attacker;
                transform.translation = ATTACKER_START_POS;
                collider.size = PLAYER_SIZE;
                *dodge_action = DodgeAction::None;
                // 移除防守方AI组件（如果存在）
                commands.entity(entity).remove::<DefenderAI>();
                // 调试输出已禁用: println!("  - 玩家 {:?} 现在是进攻方，位置: {:?}", id, ATTACKER_START_POS);
            } else {
                // 这个玩家是新的防守方
                *role = PlayerRole::Defender;
                transform.translation = DEFENDER_START_POS;
                collider.size = PLAYER_SIZE;
                *dodge_action = DodgeAction::None;
                // 调试输出已禁用: println!("  - 玩家 {:?} 现在是防守方，位置: {:?}", id, DEFENDER_START_POS);
            }
        }
        
        new_attacker
    } else {
        // 本地模式：两个玩家角色互换
        let mut new_attacker = PlayerId::Player1;
    for (entity, mut role, id, mut transform, mut collider, mut dodge_action) in query.iter_mut() {
        match *role {
            PlayerRole::Attacker => {
                    // 原来的进攻方变成防守方
                *role = PlayerRole::Defender;
                transform.translation = DEFENDER_START_POS;
                collider.size = PLAYER_SIZE;
                *dodge_action = DodgeAction::None;
                    // 调试输出已禁用: println!("  - 玩家 {:?} 从进攻方变成防守方", id);
            }
            PlayerRole::Defender => {
                    // 原来的防守方变成进攻方
                *role = PlayerRole::Attacker;
                transform.translation = ATTACKER_START_POS;
                collider.size = PLAYER_SIZE;
                *dodge_action = DodgeAction::None;
                    new_attacker = *id;
                    // 移除防守方AI组件（如果存在）
                commands.entity(entity).remove::<DefenderAI>();
                    // 调试输出已禁用: println!("  - 玩家 {:?} 从防守方变成进攻方", id);
            }
        }
    }
        new_attacker
    };
    
    // 重置回合信息
    round_info.bullets_left = BULLETS_PER_ROUND;
    round_info.round_timer.reset();
    round_info.current_attacker = new_attacker_id;
    round_info.bullets_fired_this_round = 0;
    round_info.bullets_hit_defender = 0;
    round_info.is_switching = false;
    
    // 网络模式下，立即更新视图配置并直接切换相机
    if is_network_mode {
        // 确定当前玩家的ID
        let current_player_id = if is_host { 
            crate::PlayerId::Player1 
        } else { 
            crate::PlayerId::Player2 
        };
        
        // 查询当前玩家的角色（角色已经在上面更新了）
        let mut current_player_role = None;
        for (_, role, id, _, _, _) in query.iter() {
            if *id == current_player_id {
                current_player_role = Some(*role);
                break;
            }
        }
        
        // 根据当前玩家的角色来更新视图配置并直接切换相机
        if let Some(role) = current_player_role {
            let new_is_attacker = matches!(role, PlayerRole::Attacker);
            let old_is_attacker = view_config.is_attacker_view;
            
            // 调试输出已禁用: println!("[角色切换] ========== 视图配置更新 ==========");
            // 调试输出已禁用: println!("[角色切换] 当前玩家: {:?}, 角色: {:?}", current_player_id, role);
            // 调试输出已禁用: println!("[角色切换] 旧视图配置: is_attacker_view = {} ({})", old_is_attacker, if old_is_attacker { "进攻方视图" } else { "防守方视图" });
            // 调试输出已禁用: println!("[角色切换] 新视图配置: is_attacker_view = {} ({})", new_is_attacker, if new_is_attacker { "进攻方视图" } else { "防守方视图" });
            
            // 立即更新视图配置
            view_config.is_attacker_view = new_is_attacker;
            
            // 立即发送相机切换事件
            camera_switch_writer.send(CameraSwitchEvent { is_attacker_view: new_is_attacker });
            // 调试输出已禁用: println!("[角色切换] 已发送 CameraSwitchEvent");
            
            // 标记相机状态缓存需要检查（角色切换时相机可能会改变）
            camera_state_cache.needs_check = true;
            
            // 网络模式下强制立即应用相机视图
            // 先移除相机上可能存在的 PlayerId 和 PlayerCamera 组件（这些组件不应该存在）
            // 调试输出已禁用: println!("[角色切换] ========== 开始强制切换相机 ==========");
            // 移除所有相机的 PlayerId 和 PlayerCamera 组件
            for entity in all_cameras_query.iter() {
                // 尝试移除 PlayerId 组件（如果存在）
                commands.entity(entity).remove::<PlayerId>();
                // 尝试移除 PlayerCamera 组件（如果存在）
                commands.entity(entity).remove::<PlayerCamera>();
            }
            // 直接使用 ViewConfig 中保存的相机实体ID切换，不依赖 camera_query
            // 因为 Commands 的修改是延迟执行的，所以我们需要直接操作实体
            // 优化：减少不必要的操作，只更新必要的组件
            apply_network_camera_view_direct(&mut commands, &all_cameras_query, &mut view_config, new_is_attacker);
            // 调试输出已禁用: println!("[角色切换] ========== 相机切换完成 ==========");
        } else {
            // 调试输出已禁用: println!("[错误] 未找到当前玩家 ({:?}) 的角色！", current_player_id);
            // 调试输出已禁用: println!("[错误] 所有玩家列表：");
            for (_, role, id, _, _, _) in query.iter() {
                // 调试输出已禁用: println!("  - 玩家 {:?}: 角色 {:?}", id, role);
            }
        }
    }

    // 调试输出已禁用: println!("[角色切换] 切换完成！新的进攻方: {:?}", new_attacker_id);
    
    // 网络模式下，需要清理并重新创建游戏实体（因为只创建了当前视角的实体）
    if is_network_mode {
        // 发送事件，触发游戏实体重建
        // 注意：实体重建应该在下一帧执行，因为Commands的修改是延迟执行的
        commands.insert_resource(crate::RecreateGameEntitiesOnRoleSwitch { should_recreate: true });
        // 启动角色切换缓冲计时器（防止相机在切换过程中被误删）
        commands.insert_resource(crate::RoleSwitchCooldown {
            timer: bevy::time::Timer::from_seconds(0.5, bevy::time::TimerMode::Once),
        });
        // 调试输出已禁用: println!("[角色切换] 已标记需要重新创建游戏实体，并启动缓冲计时器");
    }
    
    next_round_state.set(RoundState::Attacking);
}

/// 清理子弹系统
pub fn cleanup_bullets_on_switch(
    mut commands: Commands,
    bullet_query: Query<Entity, With<Bullet>>,
    muzzle_flash_query: Query<Entity, With<MuzzleFlash>>,
    action_timer_query: Query<Entity, With<ActionTimer>>,
) {
    for bullet_entity in bullet_query.iter() {
        commands.entity(bullet_entity).despawn();
    }
    for flash_entity in muzzle_flash_query.iter() {
        commands.entity(flash_entity).despawn();
    }
    for action_entity in action_timer_query.iter() {
        commands.entity(action_entity).despawn();
    }
}

/// 墙体可见性更新
pub fn wall_visibility_update_system(
    mut wall_segment_query: Query<(Entity, &WallSegment, &mut Sprite, &mut Visibility, &Transform, &Collider), With<WallSegment>>,
    mut wall_background_query: Query<(&WallBackground, &mut Visibility), (With<WallBackground>, Without<WallSegment>)>,
) {
    // 优化：只在有破损墙体时运行（快速检查）
    let has_damaged_walls = wall_segment_query.iter().any(|(_, segment, _, _, _, _)| segment.damaged);
    if !has_damaged_walls {
        return;
    }
    
    for (_entity, segment, mut sprite, mut visibility, segment_transform, _collider) in wall_segment_query.iter_mut() {
        if segment.damaged {
            match segment.view_layer {
                ViewLayer::AttackerView => {
                    sprite.color = Color::rgba(0.0, 0.0, 0.0, 0.0);
                    *visibility = Visibility::Hidden;
                    
                    let segment_pos = segment_transform.translation.truncate();
                    for (background, mut bg_visibility) in wall_background_query.iter_mut() {
                        if background.view_layer == ViewLayer::AttackerView {
                            let bg_pos = Vec2::new(background.position.x, WALL_POSITION.y + background.position.y);
                            if (bg_pos - segment_pos).length() < 1.0 {
                                *bg_visibility = Visibility::Hidden;
                            }
                        }
                    }
                }
                ViewLayer::DefenderView => {
                    sprite.color = Color::rgba(0.0, 0.0, 0.0, 0.8);
                    *visibility = Visibility::Visible;
                }
            }
        } else {
            *visibility = Visibility::Visible;
            
            let segment_pos = segment_transform.translation.truncate();
            for (background, mut bg_visibility) in wall_background_query.iter_mut() {
                if background.view_layer == segment.view_layer {
                    let bg_pos = Vec2::new(background.position.x, WALL_POSITION.y + background.position.y);
                    if (bg_pos - segment_pos).length() < 1.0 {
                        *bg_visibility = Visibility::Visible;
                    }
                }
            }
            let brick_width = BRICK_WIDTH;
            let brick_col = ((segment_transform.translation.x + WALL_SIZE.x / 2.0) / brick_width) as i32;
            let brick_row = ((segment_transform.translation.y - WALL_POSITION.y + WALL_SIZE.y / 2.0) / BRICK_HEIGHT) as i32;
            
            let brick_colors = vec![
                Color::rgb(0.8, 0.6, 0.5),
                Color::rgb(0.5, 0.35, 0.25),
            ];
            let brick_index = ((brick_col + brick_row) as usize) % brick_colors.len();
            sprite.color = brick_colors[brick_index];
        }
    }
}

/// 防守方可见性系统 - 简化版本：依赖Z轴顺序和墙段隐藏实现遮挡效果
/// 渲染逻辑：
/// 1. 进攻方视角：人物在Z轴1.0（先渲染），墙在Z轴2.0（后渲染，会遮挡人物）
/// 2. 破损的墙段在进攻方视角中被设置为 Visibility::Hidden，不渲染
/// 3. 通过Z轴顺序和墙段隐藏，自然实现遮挡效果：未破损的墙遮挡人物，破损的墙不遮挡
/// 4. 防守方在进攻方视角中始终可见，通过墙段隐藏实现部分可见性
pub fn defender_visibility_system(
    mut humanoid_query: Query<(&HumanoidPart, &mut Visibility), (With<HumanoidPart>, Without<PlayerId>)>,
) {
    // 进攻方视角中的所有人物（包括进攻方和防守方）始终可见
    // 通过Z轴顺序和墙段隐藏实现遮挡效果
    for (humanoid_part, mut visibility) in humanoid_query.iter_mut() {
        if humanoid_part.view_layer == ViewLayer::AttackerView {
                *visibility = Visibility::Visible;
            }
    }
    
    // 防守方视角中的所有sprite始终可见
    for (humanoid_part, mut visibility) in humanoid_query.iter_mut() {
        if humanoid_part.view_layer == ViewLayer::DefenderView {
            *visibility = Visibility::Visible;
        }
    }
}

/// 更新激光指示器
pub fn update_laser_indicator_system(
    mut laser_query: Query<(&mut Transform, &mut Sprite), With<LaserIndicator>>,
    cursor_pos: Res<CursorPosition>,
) {
    let attacker_pos = ATTACKER_START_POS.truncate();
    let target_pos = cursor_pos.0;
    let laser_direction = (target_pos - attacker_pos).normalize_or_zero();
    let laser_angle = laser_direction.y.atan2(laser_direction.x);
    
    for (mut laser_transform, mut laser_sprite) in laser_query.iter_mut() {
        let mid_point = (attacker_pos + target_pos) / 2.0;
        laser_transform.translation = mid_point.extend(50.0);
        laser_transform.rotation = Quat::from_rotation_z(laser_angle);
        
        laser_sprite.color = Color::rgba(1.0, 0.0, 0.0, 0.9);
        laser_sprite.custom_size = Some(Vec2::new(0.0, 4.0));
    }
}

/// 激光与墙重叠部分隐藏系统
pub fn update_laser_visibility_system(
    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut Sprite), (With<LaserIndicator>, Without<PlayerId>, Without<DefenderCamera>, Without<WallSegment>, Without<Bullet>)>,
        Query<(&Transform, &PlayerRole, &PlayerId), (With<PlayerId>, Without<DefenderCamera>, Without<Bullet>, Without<WallSegment>, Without<LaserIndicator>)>,
        Query<(&Transform, &Collider, &WallSegment), (With<WallSegment>, Without<PlayerId>, Without<Bullet>, Without<LaserIndicator>, Without<DefenderCamera>)>,
    )>,
    cursor_pos: Res<CursorPosition>,
    round_info: Res<RoundInfo>,
) {
    const TRUNCATE_OFFSET: f32 = 3.0;
    const MIN_LASER_LENGTH: f32 = 5.0;

    let attacker_query = param_set.p1();
    let mut attacker_pos = ATTACKER_START_POS.truncate();
    let mut found_attacker = false;
    for (transform, role, id) in attacker_query.iter() {
        if matches!(role, PlayerRole::Attacker) && *id == round_info.current_attacker {
            attacker_pos = transform.translation.truncate();
            found_attacker = true;
            break;
        }
    }
    if !found_attacker {
        let mut laser_query = param_set.p0();
        for (_, mut laser_sprite) in laser_query.iter_mut() {
            laser_sprite.custom_size = Some(Vec2::new(0.0, 4.0));
        }
        return;
    }

    let target_pos = cursor_pos.0;
    let laser_vec = target_pos - attacker_pos;
    let laser_direction = laser_vec.normalize_or_zero();
    let full_laser_length = laser_vec.length().max(MIN_LASER_LENGTH);
    let mut visible_laser_length = full_laser_length;

    let wall_segment_query = param_set.p2();
    for (wall_transform, wall_collider, wall_segment) in wall_segment_query.iter() {
        if wall_segment.damaged {
            continue;
        }
        let wall_pos = wall_transform.translation.truncate();
        let wall_half_size = wall_collider.size / 2.0;
        let wall_rect = (wall_pos - wall_half_size, wall_pos + wall_half_size);

        if laser_segment_rect_intersects(attacker_pos, target_pos, wall_rect) {
            if let Some(intersection) = laser_line_rect_intersection(attacker_pos, target_pos, wall_rect) {
                let distance = (intersection - attacker_pos).length();
                if distance > 0.0 && distance < visible_laser_length {
                    visible_laser_length = (distance - TRUNCATE_OFFSET).max(MIN_LASER_LENGTH);
                }
            }
        }
    }

    let mut laser_query = param_set.p0();
    for (mut laser_transform, mut laser_sprite) in laser_query.iter_mut() {
        let visible_end = attacker_pos + laser_direction * visible_laser_length;
        let visible_mid = (attacker_pos + visible_end) / 2.0;
        
        laser_transform.translation = visible_mid.extend(50.0);
        laser_sprite.custom_size = Some(Vec2::new(visible_laser_length, 4.0));
    }
}

/// 音频资源管理器
#[derive(Resource)]
pub struct SoundEffects {
    pub die_sounds: Vec<Handle<bevy::audio::AudioSource>>,      // 射中身子/脚音效（随机播放，双方可听到）
    pub head_sounds: Vec<Handle<bevy::audio::AudioSource>>,    // 射中头部音效（随机播放，双方可听到）
    pub lose_sounds: Vec<Handle<bevy::audio::AudioSource>>,     // 失败音效（随机播放，仅输家听到）
    pub win_sounds: Vec<Handle<bevy::audio::AudioSource>>,      // 胜利音效（随机播放，仅胜者听到）
    pub hh_sounds: Vec<Handle<bevy::audio::AudioSource>>,      // 数字键音效（1-5对应，双方可听到）
}

/// 预加载音效资源
pub fn preload_sound_effects(
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    // 加载die音效（1.mp3, 2.mp3, 3.mp3）- 随机播放，双方可听到
    let die_sounds = vec![
        asset_server.load("music/die/1.mp3"),
        asset_server.load("music/die/2.mp3"),
        asset_server.load("music/die/3.mp3"),
    ];
    
    // 加载head音效（1.mp3）- 随机播放，双方可听到
    let head_sounds = vec![
        asset_server.load("music/head/1.mp3"),
    ];
    
    // 加载lose音效（1.mp3, 2.mp3）- 随机播放，仅输家听到
    let lose_sounds = vec![
        asset_server.load("music/lose/1.mp3"),
        asset_server.load("music/lose/2.mp3"),
    ];
    
    // 加载win音效（1.mp3）- 随机播放，仅胜者听到
    let win_sounds = vec![
        asset_server.load("music/win/1.mp3"),
    ];
    
    // 加载hh音效（1.mp3, 2.mp3, 3.mp3, 4.mp3, 5.mp3）- 数字键1-5对应，双方可听到
    let hh_sounds = vec![
        asset_server.load("music/hh/1.mp3"),
        asset_server.load("music/hh/2.mp3"),
        asset_server.load("music/hh/3.mp3"),
        asset_server.load("music/hh/4.mp3"),
        asset_server.load("music/hh/5.mp3"),
    ];
    
    commands.insert_resource(SoundEffects {
        die_sounds,
        head_sounds,
        lose_sounds,
        win_sounds,
        hh_sounds,
    });
    
    // 调试输出已禁用: println!("[音效] 音效资源已预加载");
}

/// 播放音效（一次性播放）
fn play_sound_effect(
    commands: &mut Commands,
    sound_handle: Handle<bevy::audio::AudioSource>,
) {
    commands.spawn(AudioBundle {
        source: sound_handle,
        settings: PlaybackSettings::ONCE,
        ..default()
    });
}

/// 处理玩家被击中事件（播放音效）
pub fn handle_player_hit_event(
    mut reader: EventReader<PlayerHitEvent>,
    sound_effects: Option<Res<SoundEffects>>,
    mut commands: Commands,
) {
    for event in reader.read() {
        // 调试输出已禁用: println!("Hit {:?} in {:?}! Damage: {}", event.player_id, event.hitbox_type, event.damage);
        
        // 播放音效（双方都能听到）
        if let Some(sounds) = sound_effects.as_ref() {
            match event.hitbox_type {
                HitboxType::Head => {
                    // 射中头部：随机播放head音效（双方可听到）
                    if !sounds.head_sounds.is_empty() {
                        let index = rand::thread_rng().gen_range(0..sounds.head_sounds.len());
                        play_sound_effect(&mut commands, sounds.head_sounds[index].clone());
                        // 调试输出已禁用: println!("[音效] 播放头部命中音效");
                    }
                }
                HitboxType::Torso | HitboxType::Legs => {
                    // 射中身子/脚：随机播放die音效（双方可听到）
                    if !sounds.die_sounds.is_empty() {
                        let index = rand::thread_rng().gen_range(0..sounds.die_sounds.len());
                        play_sound_effect(&mut commands, sounds.die_sounds[index].clone());
                        // 调试输出已禁用: println!("[音效] 播放身体命中音效");
                    }
                }
            }
        }
    }
}

/// 处理游戏结束事件（播放音效）
pub fn handle_game_over_event(
    mut reader: EventReader<GameOverEvent>,
    sound_effects: Option<Res<SoundEffects>>,
    mut commands: Commands,
    room_info: Option<Res<crate::RoomInfo>>,
) {
    for event in reader.read() {
        // 调试输出已禁用: println!("Final Result: Winner {:?}, Loser {:?}", event.winner_id, event.loser_id);
        
        // 播放音效（仅对应玩家听到）
        if let Some(sounds) = sound_effects.as_ref() {
            // 确定本地玩家ID
            let local_player_id = if let Some(room_info) = room_info.as_ref() {
                if room_info.is_host {
                    crate::PlayerId::Player1
                } else {
                    crate::PlayerId::Player2
                }
            } else {
                // 本地模式，假设是Player1
                crate::PlayerId::Player1
            };
            
            // 如果本地玩家是获胜者，随机播放win音效（仅胜者听到）
            if event.winner_id == local_player_id {
                if !sounds.win_sounds.is_empty() {
                    let index = rand::thread_rng().gen_range(0..sounds.win_sounds.len());
                    play_sound_effect(&mut commands, sounds.win_sounds[index].clone());
                    // 调试输出已禁用: println!("[音效] 播放胜利音效");
                }
            }
            
            // 如果本地玩家是失败者，随机播放lose音效（仅输家听到）
            if event.loser_id == local_player_id {
                if !sounds.lose_sounds.is_empty() {
                    let index = rand::thread_rng().gen_range(0..sounds.lose_sounds.len());
                    play_sound_effect(&mut commands, sounds.lose_sounds[index].clone());
                    // 调试输出已禁用: println!("[音效] 播放失败音效");
                }
            }
        }
    }
}

/// 处理数字键音效（12345对应hh音效，双方可听到）
pub fn handle_number_key_sound_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    sound_effects: Option<Res<SoundEffects>>,
    mut commands: Commands,
) {
    if let Some(sounds) = sound_effects.as_ref() {
        // 检查数字键1-5（双方都能听到）
        if keyboard_input.just_pressed(KeyCode::Digit1) && sounds.hh_sounds.len() > 0 {
            play_sound_effect(&mut commands, sounds.hh_sounds[0].clone());
            // 调试输出已禁用: println!("[音效] 播放数字键1音效");
        } else if keyboard_input.just_pressed(KeyCode::Digit2) && sounds.hh_sounds.len() > 1 {
            play_sound_effect(&mut commands, sounds.hh_sounds[1].clone());
            // 调试输出已禁用: println!("[音效] 播放数字键2音效");
        } else if keyboard_input.just_pressed(KeyCode::Digit3) && sounds.hh_sounds.len() > 2 {
            play_sound_effect(&mut commands, sounds.hh_sounds[2].clone());
            // 调试输出已禁用: println!("[音效] 播放数字键3音效");
        } else if keyboard_input.just_pressed(KeyCode::Digit4) && sounds.hh_sounds.len() > 3 {
            play_sound_effect(&mut commands, sounds.hh_sounds[3].clone());
            // 调试输出已禁用: println!("[音效] 播放数字键4音效");
        } else if keyboard_input.just_pressed(KeyCode::Digit5) && sounds.hh_sounds.len() > 4 {
            play_sound_effect(&mut commands, sounds.hh_sounds[4].clone());
            // 调试输出已禁用: println!("[音效] 播放数字键5音效");
        }
    }
}

/// 处理玩家动作事件
pub fn handle_player_action_event(
    mut reader: EventReader<PlayerActionEvent>,
) {
    for event in reader.read() {
        // 调试输出已禁用: println!("Player {:?} used action: {:?}", event.player_id, event.action);
    }
}

// --- 辅助函数 ---

/// AABB轴对齐包围盒碰撞检测
fn check_collision(pos1: Vec3, size1: Vec2, pos2: Vec3, size2: Vec2) -> bool {
    let half_size1 = size1 / 2.0;
    let half_size2 = size2 / 2.0;
    
    pos1.x < pos2.x + half_size2.x &&
    pos1.x + half_size1.x > pos2.x &&
    pos1.y < pos2.y + half_size2.y &&
    pos1.y + half_size1.y > pos2.y
}

/// 线段与线段精确交点计算
fn line_line_intersection(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Option<Vec2> {
    fn cross(v1: Vec2, v2: Vec2) -> f32 {
        v1.x * v2.y - v1.y * v2.x
    }

    let d1 = a2 - a1;
    let d2 = b2 - b1;
    let d = b1 - a1;

    let denom = cross(d1, d2);
    if denom.abs() < 1e-6 {
        return None;
    }

    let t = cross(d, d2) / denom;
    let u = cross(d, d1) / denom;

    if t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0 {
        Some(a1 + t * d1)
    } else {
        None
    }
}

/// 激光专用线段-线段相交判断
fn laser_segments_intersect(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> bool {
    line_line_intersection(a1, a2, b1, b2).is_some()
}

/// 激光专用线段-轴对齐矩形相交判断
fn laser_segment_rect_intersects(p1: Vec2, p2: Vec2, rect: (Vec2, Vec2)) -> bool {
    let (rect_min, rect_max) = rect;

    let seg_min = Vec2::new(p1.x.min(p2.x), p1.y.min(p2.y));
    let seg_max = Vec2::new(p1.x.max(p2.x), p1.y.max(p2.y));
    if seg_max.x < rect_min.x || seg_min.x > rect_max.x || seg_max.y < rect_min.y || seg_min.y > rect_max.y {
        return false;
    }

    let edges = [
        (Vec2::new(rect_min.x, rect_max.y), Vec2::new(rect_max.x, rect_max.y)),
        (Vec2::new(rect_min.x, rect_min.y), Vec2::new(rect_max.x, rect_min.y)),
        (Vec2::new(rect_min.x, rect_min.y), Vec2::new(rect_min.x, rect_max.y)),
        (Vec2::new(rect_max.x, rect_min.y), Vec2::new(rect_max.x, rect_max.y)),
    ];

    for (b1, b2) in edges {
        if laser_segments_intersect(p1, p2, b1, b2) {
            return true;
        }
    }

    fn point_in_rect(p: Vec2, rect: (Vec2, Vec2)) -> bool {
        let (min, max) = rect;
        p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y
    }
    if point_in_rect(p1, rect) || point_in_rect(p2, rect) {
        return true;
    }

    false
}

/// 激光专用：计算线段与矩形的最近交点
fn laser_line_rect_intersection(p1: Vec2, p2: Vec2, rect: (Vec2, Vec2)) -> Option<Vec2> {
    let (rect_min, rect_max) = rect;
    let mut closest_intersection: Option<Vec2> = None;
    let mut min_distance = f32::INFINITY;

    let edges = [
        (Vec2::new(rect_min.x, rect_max.y), Vec2::new(rect_max.x, rect_max.y)),
        (Vec2::new(rect_min.x, rect_min.y), Vec2::new(rect_max.x, rect_min.y)),
        (Vec2::new(rect_min.x, rect_min.y), Vec2::new(rect_min.x, rect_max.y)),
        (Vec2::new(rect_max.x, rect_min.y), Vec2::new(rect_max.x, rect_max.y)),
    ];

    for (b1, b2) in edges {
        if let Some(intersect) = line_line_intersection(p1, p2, b1, b2) {
            let distance = (intersect - p1).length();
            if distance < min_distance && distance > 1e-3 {
                min_distance = distance;
                closest_intersection = Some(intersect);
            }
        }
    }

    closest_intersection
}

/// 线段-矩形碰撞检测
pub fn check_line_collision(start: Vec2, end: Vec2, rect_pos: Vec2, rect_size: Vec2) -> bool {
    let rect_min = rect_pos - rect_size / 2.0;
    let rect_max = rect_pos + rect_size / 2.0;
    laser_segment_rect_intersects(start, end, (rect_min, rect_max))
}

// --- UI 系统 ---

/// 更新UI（只在子弹数变化时运行）
pub fn update_ui(
    round_info: Res<RoundInfo>,
    mut bullet_icon_query: Query<(&BulletIcon, &mut Visibility), With<BulletIcon>>,
    mut ui_state: ResMut<UiStateTracker>,
) {
    // 只在子弹数变化时更新
    if ui_state.last_bullets_left == round_info.bullets_left {
        return;
    }
    ui_state.last_bullets_left = round_info.bullets_left;
    
    for (bullet_icon, mut visibility) in bullet_icon_query.iter_mut() {
        *visibility = if bullet_icon.index < round_info.bullets_left as usize {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// 更新血量显示（同时更新进攻方和防守方视角的血量）
/// 注意：网络模式下，防守方也应该显示双方血量
/// 优化：只在血量变化时运行
pub fn update_health_display(
    round_info: Res<RoundInfo>,
    mut health_display_query: Query<(&PlayerHealthDisplay, &mut Text), (With<PlayerHealthDisplay>, Without<BulletIcon>)>,
    font_resource: Res<crate::FontResource>,
    mut ui_state: ResMut<UiStateTracker>,
) {
    // 只在血量变化时更新
    let p1_health = round_info.p1_health.max(0.0);
    let p2_health = round_info.p2_health.max(0.0);
    if (ui_state.last_p1_health - p1_health).abs() < 0.1 && (ui_state.last_p2_health - p2_health).abs() < 0.1 {
        return;
    }
    ui_state.last_p1_health = p1_health;
    ui_state.last_p2_health = p2_health;
    
    let font = font_resource.font.clone();
    
    for (display, mut text) in health_display_query.iter_mut() {
        let health = match display.player_id {
            PlayerId::Player1 => p1_health,
            PlayerId::Player2 => p2_health,
        };
        // 更新字体（确保使用正确的字体）
        text.sections[1].style.font = font.clone();
        text.sections[1].value = format!("{:.0}", health);
        
        // 注意：血量显示应该始终可见，不受视图配置影响
        // 因为 update_network_ui_visibility 会控制整个 DefenderUI 容器的显示/隐藏
    }
}

/// 更新动作冷却显示（优化：只在冷却时间变化时运行）
pub fn update_action_cooldown_display(
    time: Res<Time>,
    query: Query<(&PlayerRole, &ActionCooldown, &PlayerId)>,
    mut cooldown_text_query: Query<&mut Text, With<ActionCooldownText>>,
    font_resource: Res<crate::FontResource>,
    mut ui_state: ResMut<UiStateTracker>,
) {
    let font = font_resource.font.clone();
    let mut defender_cooldown = None;
    for (role, cooldown, player_id) in query.iter() {
        if matches!(role, PlayerRole::Defender) {
            defender_cooldown = Some((cooldown, player_id));
            break;
        }
    }
    
    if let Some((cooldown, player_id)) = defender_cooldown {
        let current_time = time.elapsed_seconds_f64();
        let time_since_last_action = current_time - cooldown.last_action_time;
        let remaining_cooldown = cooldown.cooldown_duration - time_since_last_action;
        
        let new_text = if remaining_cooldown <= 0.0 {
            "就绪".to_string()
        } else {
            format!("{:.1}秒", remaining_cooldown)
        };
        
        // 只在文本变化时更新（避免每帧更新）
        if ui_state.last_cooldown_text == new_text {
            return;
        }
        ui_state.last_cooldown_text = new_text.clone();
        
        if let Ok(mut text) = cooldown_text_query.get_single_mut() {
            // 更新字体
            text.sections[0].style.font = font.clone();
            text.sections[1].style.font = font.clone();
            
            if remaining_cooldown <= 0.0 {
                text.sections[1].value = new_text;
                text.sections[1].style.color = Color::GREEN;
            } else {
                text.sections[1].value = new_text;
                text.sections[1].style.color = Color::RED;
            }
            text.sections[0].value = format!("P{:?} 动作冷却: ", player_id);
        }
    }
}

// --- 游戏结束系统 ---

/// 游戏结束界面
pub fn setup_gameover_screen(
    mut commands: Commands,
    mut reader: EventReader<GameOverEvent>,
    font_resource: Res<crate::FontResource>,
    room_info: Option<Res<crate::RoomInfo>>,
    ui_camera_entities: Option<Res<crate::UiCameraEntities>>,
    mut camera_query: Query<(Entity, &mut Camera), (With<Camera2d>, With<IsDefaultUiCamera>)>,
    defender_ui_root_query: Query<Entity, With<crate::DefenderUIRoot>>,
    game_over_delay: Option<Res<GameOverDelay>>,
) {
    // 首先尝试从事件读取，如果读取不到，从资源读取
    let game_over_event = if let Some(event) = reader.read().next() {
        // 调试输出已禁用: println!("[游戏结束调试] 收到GameOverEvent: winner={:?}, loser={:?}", event.winner_id, event.loser_id);
        GameOverEvent {
            winner_id: event.winner_id,
            loser_id: event.loser_id,
        }
    } else if let Some(delay) = game_over_delay.as_ref() {
        // 如果事件读取不到，尝试从资源读取
        if let (Some(winner_id), Some(loser_id)) = (delay.winner_id, delay.loser_id) {
            // 调试输出已禁用: println!("[游戏结束调试] 未找到GameOverEvent，从GameOverDelay资源读取: winner={:?}, loser={:?}", winner_id, loser_id);
            GameOverEvent {
                winner_id,
                loser_id,
            }
        } else {
            // 调试输出已禁用: println!("[游戏结束调试] 错误：未找到 GameOverEvent 且 GameOverDelay 资源中没有游戏结束信息！");
            return;
        }
    } else {
        // 调试输出已禁用: println!("[游戏结束调试] 错误：未找到 GameOverEvent 且 GameOverDelay 资源不存在！");
        return;
    };
    
    let is_network_mode = room_info.as_ref().map(|r| r.is_connected).unwrap_or(false);
    let is_host = room_info.as_ref().map(|r| r.is_host).unwrap_or(false);
    
    // 调试输出已禁用: println!("[游戏结束调试] ========== 设置游戏结束界面 ==========");
    // 调试输出已禁用: println!("[游戏结束调试] 获胜者: {:?}, 失败者: {:?}", game_over_event.winner_id, game_over_event.loser_id);
    // 调试输出已禁用: println!("[游戏结束调试] 网络模式: {}, 房主: {}", is_network_mode, is_host);
    
    // 初始化再来一局状态
    commands.insert_resource(RematchState::default());
    
    let winner_text = match game_over_event.winner_id {
        PlayerId::Player1 => "玩家1",
        PlayerId::Player2 => "玩家2",
    };
    // 调试输出已禁用: println!("[游戏结束调试] 获胜者文本: {}", winner_text);
    
    let font = font_resource.font.clone();
    
    let text_style = TextStyle {
        font: font.clone(),
        font_size: 64.0,
        color: Color::YELLOW,
    };
    let sub_text_style = TextStyle {
        font: font.clone(),
        font_size: 32.0,
        color: Color::WHITE,
    };
    let hint_text_style = TextStyle {
        font: font.clone(),
        font_size: 24.0,
        color: Color::GRAY,
    };
    let button_text_style = TextStyle {
        font: font.clone(),
        font_size: 36.0,
        color: Color::WHITE,
    };
    
    // 游戏结束弹窗应该覆盖所有UI（包括进攻方和防守方的UI）
    // 使用更高的ZIndex确保在所有UI之上显示
    // 注意：不使用AttackerUI或DefenderUI标记，确保在所有视角下都能显示
    // 在游戏结束时，确保防守方UI相机也能看到游戏结束屏幕
    // 通过不设置任何特定的UI相机标记，让UI渲染到所有有IsDefaultUiCamera的相机
    // 调试输出已禁用: println!("[游戏结束调试] 创建主游戏结束屏幕（进攻方/默认）");
    let game_over_entity = commands.spawn(NodeBundle {
        style: Style {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Val::Px(20.0),
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            ..default()
        },
        background_color: Color::rgba(0.0, 0.0, 0.0, 0.9).into(),
        z_index: ZIndex::Global(300), // 使用更高的ZIndex，确保在所有UI之上
        ..default()
    }).with_children(|parent| {
        // 游戏结束文本
        parent.spawn(TextBundle::from_sections([
            TextSection::new("游戏结束\n\n".to_string(), text_style.clone()),
            TextSection::new(format!("获胜者: {}\n\n", winner_text), sub_text_style.clone()),
        ]));
        
        // 按钮容器（网络模式）
        if is_network_mode {
            parent.spawn(NodeBundle {
                style: Style {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(20.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    margin: UiRect::top(Val::Px(20.0)),
                    ..default()
                },
                ..default()
            }).with_children(|buttons| {
                // 再来一局按钮
                buttons.spawn(ButtonBundle {
                    style: Style {
                        width: Val::Px(200.0),
                        height: Val::Px(60.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::rgb(0.2, 0.6, 0.2).into(),
                    ..default()
                }).insert(RematchButton).with_children(|button| {
                    button.spawn(TextBundle::from_section("再来一局", button_text_style.clone()));
                });
                
                // 返回大厅按钮
                buttons.spawn(ButtonBundle {
                    style: Style {
                        width: Val::Px(200.0),
                        height: Val::Px(60.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::rgb(0.4, 0.4, 0.6).into(),
                    ..default()
                }).insert(ReturnToLobbyButton).with_children(|button| {
                    button.spawn(TextBundle::from_section("返回大厅", button_text_style.clone()));
                });
            });
            
            // 提示文本
            parent.spawn(TextBundle::from_section("等待对方选择...", hint_text_style));
        } else {
            // 本地模式提示
            parent.spawn(TextBundle::from_section("按 R 键重新开始\n按 Q 键退出游戏", hint_text_style));
        }
    }).id();
    
    // 在游戏结束时，确保游戏结束屏幕能被防守方的UI相机看到
    // 网络模式下，如果防守方有单独的UI相机，需要为防守方也创建一个游戏结束屏幕
    // 因为Bevy的UI系统只会将UI渲染到一个IsDefaultUiCamera相机
    if let Some(ui_cameras) = ui_camera_entities.as_ref() {
        let is_network_mode = room_info.as_ref().map(|r| r.is_connected).unwrap_or(false);
        // 调试输出已禁用: println!("[游戏结束调试] 网络模式: {}, 防守方UI相机: {:?}", is_network_mode, ui_cameras.defender_ui_camera);
        if is_network_mode && ui_cameras.defender_ui_camera != Entity::PLACEHOLDER {
            // 为防守方创建单独的游戏结束屏幕
            // 方法：将游戏结束屏幕添加到防守方的UI根节点下，这样它会自动渲染到防守方的UI相机
            // 调试输出已禁用: println!("[游戏结束调试] 尝试查找防守方UI根节点...");
            let defender_ui_root_result = defender_ui_root_query.get_single();
            if let Ok(defender_ui_root) = defender_ui_root_result {
                // 调试输出已禁用: println!("[游戏结束调试] 找到防守方UI根节点: {:?}", defender_ui_root);
                // 找到防守方的UI根节点，将游戏结束屏幕作为其子节点
                commands.entity(defender_ui_root).with_children(|parent| {
                    parent.spawn((
                        NodeBundle {
                            style: Style {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                display: Display::Flex,
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                row_gap: Val::Px(20.0),
                                position_type: PositionType::Absolute,
                                top: Val::Px(0.0),
                                left: Val::Px(0.0),
                                ..default()
                            },
                            background_color: Color::rgba(0.0, 0.0, 0.0, 0.9).into(),
                            z_index: ZIndex::Global(300),
                            ..default()
                        },
                    )).with_children(|game_over_parent| {
                        game_over_parent.spawn(TextBundle::from_sections([
                            TextSection::new("游戏结束\n\n".to_string(), text_style.clone()),
                            TextSection::new(format!("获胜者: {}\n\n", winner_text), sub_text_style.clone()),
                        ]));
                        
                        game_over_parent.spawn(NodeBundle {
                            style: Style {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(20.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                margin: UiRect::top(Val::Px(20.0)),
                                ..default()
                            },
                            ..default()
                        }).with_children(|buttons| {
                            buttons.spawn(ButtonBundle {
                                style: Style {
                                    width: Val::Px(200.0),
                                    height: Val::Px(60.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                background_color: Color::rgb(0.2, 0.6, 0.2).into(),
                                ..default()
                            }).insert(RematchButton).with_children(|button| {
                                button.spawn(TextBundle::from_section("再来一局", button_text_style.clone()));
                            });
                            
                            buttons.spawn(ButtonBundle {
                                style: Style {
                                    width: Val::Px(200.0),
                                    height: Val::Px(60.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                background_color: Color::rgb(0.4, 0.4, 0.6).into(),
                                ..default()
                            }).insert(ReturnToLobbyButton).with_children(|button| {
                                button.spawn(TextBundle::from_section("返回大厅", button_text_style.clone()));
                            });
                        });
                        
                        let defender_hint_text_style = TextStyle {
                            font: font.clone(),
                            font_size: 24.0,
                            color: Color::GRAY,
                        };
                        game_over_parent.spawn(TextBundle::from_section("等待对方选择...", defender_hint_text_style));
                    });
                });
                // 调试输出已禁用: println!("[游戏结束调试] 成功将游戏结束屏幕添加到防守方UI根节点下");
            } else {
                // 如果找不到防守方的UI根节点，创建一个独立的游戏结束屏幕
                // 调试输出已禁用: println!("[游戏结束调试] 警告：找不到防守方UI根节点！错误: {:?}", defender_ui_root_result);
                // 调试输出已禁用: println!("[游戏结束调试] 创建独立的游戏结束屏幕作为后备方案");
                commands.spawn((
                    NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(20.0),
                            position_type: PositionType::Absolute,
                            top: Val::Px(0.0),
                            left: Val::Px(0.0),
                            ..default()
                        },
                        background_color: Color::rgba(0.0, 0.0, 0.0, 0.9).into(),
                        z_index: ZIndex::Global(300),
                        ..default()
                    },
                )).with_children(|parent| {
                parent.spawn(TextBundle::from_sections([
                    TextSection::new("游戏结束\n\n".to_string(), text_style.clone()),
                    TextSection::new(format!("获胜者: {}\n\n", winner_text), sub_text_style.clone()),
                ]));
                
                parent.spawn(NodeBundle {
                    style: Style {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(20.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        margin: UiRect::top(Val::Px(20.0)),
                        ..default()
                    },
                    ..default()
                }).with_children(|buttons| {
                    buttons.spawn(ButtonBundle {
                        style: Style {
                            width: Val::Px(200.0),
                            height: Val::Px(60.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::rgb(0.2, 0.6, 0.2).into(),
                        ..default()
                    }).insert(RematchButton).with_children(|button| {
                        button.spawn(TextBundle::from_section("再来一局", button_text_style.clone()));
                    });
                    
                    buttons.spawn(ButtonBundle {
                        style: Style {
                            width: Val::Px(200.0),
                            height: Val::Px(60.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::rgb(0.4, 0.4, 0.6).into(),
                        ..default()
                    }).insert(ReturnToLobbyButton).with_children(|button| {
                        button.spawn(TextBundle::from_section("返回大厅", button_text_style.clone()));
                    });
                });
                
                let defender_hint_text_style = TextStyle {
                    font: font.clone(),
                    font_size: 24.0,
                    color: Color::GRAY,
                };
                parent.spawn(TextBundle::from_section("等待对方选择...", defender_hint_text_style));
            });
            }
            
            // 确保防守方的UI相机有IsDefaultUiCamera，这样游戏结束屏幕会自动渲染到它
            // 防守方的UI相机在创建时已经有IsDefaultUiCamera，但为了确保，我们再次检查
            // 注意：如果实体不存在，跳过（避免panic）
            if ui_cameras.defender_ui_camera != Entity::PLACEHOLDER {
                let camera_check_result = camera_query.get(ui_cameras.defender_ui_camera);
                if camera_check_result.is_err() {
                    // 如果查询失败，可能是实体不存在或没有IsDefaultUiCamera组件
                    // 尝试直接添加（如果实体存在）
                    if let Some(_) = commands.get_entity(ui_cameras.defender_ui_camera) {
                        commands.entity(ui_cameras.defender_ui_camera).insert(IsDefaultUiCamera);
                    }
                }
            }
        } else {
            // 调试输出已禁用: println!("[游戏结束调试] 跳过防守方游戏结束屏幕创建：网络模式={}, 防守方UI相机={:?}", is_network_mode, ui_cameras.defender_ui_camera);
        }
    } else {
        // 调试输出已禁用: println!("[游戏结束调试] 没有UiCameraEntities资源");
    }
}

/// 再来一局按钮组件
#[derive(Component)]
pub struct RematchButton;

/// 返回大厅按钮组件
#[derive(Component)]
pub struct ReturnToLobbyButton;

/// 游戏结束输入处理
pub fn handle_gameover_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut app_state: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut rematch_state: ResMut<RematchState>,
    room_info: Option<Res<crate::RoomInfo>>,
    network_manager: Option<Res<crate::network_game::NetworkManager>>,
    mut button_query: Query<(&Interaction, &mut BackgroundColor, Option<&RematchButton>, Option<&ReturnToLobbyButton>), Changed<Interaction>>,
    mut next_round_state: ResMut<NextState<RoundState>>,
) {
    let is_network_mode = room_info.as_ref().map(|r| r.is_connected).unwrap_or(false);
    
    // 处理按钮点击（合并查询以避免冲突）
    if is_network_mode {
        for (interaction, mut bg_color, rematch_button, return_lobby_button) in button_query.iter_mut() {
            if let Some(_) = rematch_button {
                // 再来一局按钮
                match *interaction {
                    Interaction::Pressed => {
                        // 发送再来一局请求
                        if let Some(nm) = network_manager.as_ref() {
                            let is_host = room_info.as_ref().map(|r| r.is_host).unwrap_or(false);
                            if is_host {
                                rematch_state.host_ready = true;
                                // 调试输出已禁用: println!("[房主] 点击了再来一局");
                            } else {
                                rematch_state.client_ready = true;
                                crate::network_game::send_network_message(&**nm, crate::network_game::NetworkMessage::RematchRequest);
                                // 调试输出已禁用: println!("[客户端] 点击了再来一局，发送请求");
                            }
                        }
                        *bg_color = Color::rgb(0.3, 0.7, 0.3).into();
                    }
                    Interaction::Hovered => {
                        *bg_color = Color::rgb(0.3, 0.7, 0.3).into();
                    }
                    Interaction::None => {
                        *bg_color = Color::rgb(0.2, 0.6, 0.2).into();
                    }
                }
            } else if let Some(_) = return_lobby_button {
                // 返回大厅按钮
                match *interaction {
                    Interaction::Pressed => {
                        // 调试输出已禁用: println!("[游戏结束] 返回大厅");
                        // 清理游戏状态
                        commands.remove_resource::<RoundInfo>();
                        commands.remove_resource::<RematchState>();
                        // 返回主菜单
                        app_state.set(AppState::MainMenu);
                        *bg_color = Color::rgb(0.5, 0.5, 0.7).into();
                    }
                    Interaction::Hovered => {
                        *bg_color = Color::rgb(0.5, 0.5, 0.7).into();
                    }
                    Interaction::None => {
                        *bg_color = Color::rgb(0.4, 0.4, 0.6).into();
                    }
                }
            }
        }
        
        // 检查双方是否都准备好了
        if rematch_state.host_ready && rematch_state.client_ready {
            // 调试输出已禁用: println!("[再来一局] 双方都准备好了，重新开始游戏");
            // 重置游戏状态
            commands.remove_resource::<RoundInfo>();
            commands.remove_resource::<RematchState>();
            // 重新开始游戏
            app_state.set(AppState::Playing);
            next_round_state.set(RoundState::Attacking);
        }
    } else {
        // 本地模式：按R键重新开始
        if keyboard_input.just_pressed(KeyCode::KeyR) {
            commands.remove_resource::<RoundInfo>();
            app_state.set(AppState::MainMenu);
        } else if keyboard_input.just_pressed(KeyCode::KeyQ) {
            std::process::exit(0);
        }
    }
}

/// 处理游戏结束网络消息（客户端收到游戏结束消息后触发）
pub fn handle_game_over_network_system(
    network_manager: Option<Res<crate::network_game::NetworkManager>>,
    room_info: Option<Res<crate::RoomInfo>>,
    mut game_over_delay: ResMut<GameOverDelay>,
) {
    let Some(room_info) = room_info.as_ref() else {
        return;
    };
    
    if !room_info.is_connected {
        return;
    }
    
    let Some(nm) = network_manager.as_ref() else {
        return;
    };
    
    // 只有客户端才处理（房主已经在本地触发了游戏结束）
    if nm.is_host {
        return;
    }
    
    // 调试输出已禁用: println!("[游戏结束调试] 客户端检查游戏结束网络消息...");
    
    // 处理游戏结束消息（启动延迟计时器，不立即显示）
    if let Ok(mut queue) = nm.message_queue.lock() {
        let mut messages_to_keep = Vec::new();
        let mut found_game_over = false;
        for msg in queue.drain(..) {
            match msg {
                crate::network_game::NetworkMessage::GameOver { winner } => {
                    // 调试输出已禁用: println!("[游戏结束调试] 客户端收到GameOver网络消息: winner={:?}", winner);
                    found_game_over = true;
                    // 确定失败者
                    let loser = match winner {
                        crate::PlayerId::Player1 => crate::PlayerId::Player2,
                        crate::PlayerId::Player2 => crate::PlayerId::Player1,
                    };
                    
                    // 调试输出已禁用: println!("[游戏结束调试] 客户端启动延迟计时器（2秒）");
                    // 启动延迟计时器（2秒）
                    game_over_delay.timer = Some(Timer::from_seconds(2.0, TimerMode::Once));
                    game_over_delay.winner_id = Some(winner);
                    game_over_delay.loser_id = Some(loser);
                    
                    // 不立即发送事件和切换状态，延迟系统会在2秒后处理
                    // game_over_events.send(GameOverEvent {
                    //     winner_id: winner,
                    //     loser_id: loser,
                    // });
                    // app_state.set(AppState::GameOver);
                }
                _ => {
                    messages_to_keep.push(msg);
                }
            }
        }
        // 调试输出已禁用: if !found_game_over {
        //     println!("[游戏结束调试] 客户端未找到GameOver网络消息，消息队列中有 {} 条消息", messages_to_keep.len());
        // }
        queue.extend(messages_to_keep);
    } else {
        // 调试输出已禁用: println!("[游戏结束调试] 错误：客户端无法锁定消息队列");
    }
}

/// 处理再来一局网络消息
pub fn handle_rematch_system(
    mut rematch_state: ResMut<RematchState>,
    network_manager: Option<Res<crate::network_game::NetworkManager>>,
    room_info: Option<Res<crate::RoomInfo>>,
    mut commands: Commands,
    mut app_state: ResMut<NextState<AppState>>,
    mut next_round_state: ResMut<NextState<RoundState>>,
) {
    let Some(room_info) = room_info.as_ref() else {
        return;
    };
    
    if !room_info.is_connected {
        return;
    }
    
    let Some(nm) = network_manager.as_ref() else {
        return;
    };
    
    // 处理再来一局消息
    if let Ok(mut queue) = nm.message_queue.lock() {
        let mut messages_to_keep = Vec::new();
        for msg in queue.drain(..) {
            match msg {
                crate::network_game::NetworkMessage::RematchRequest => {
                    // 房主收到客户端再来一局请求
                    if nm.is_host {
                        rematch_state.client_ready = true;
                        // 调试输出已禁用: println!("[房主] 客户端已点击再来一局");
                        // 发送准备消息给客户端
                        crate::network_game::send_network_message(&**nm, crate::network_game::NetworkMessage::RematchReady);
                    }
                }
                crate::network_game::NetworkMessage::RematchReady => {
                    // 客户端收到房主准备消息
                    if !nm.is_host {
                        rematch_state.host_ready = true;
                        // 调试输出已禁用: println!("[客户端] 房主已点击再来一局");
                    }
                }
                _ => {
                    messages_to_keep.push(msg);
                }
            }
        }
        queue.extend(messages_to_keep);
    }
    
    // 检查双方是否都准备好了
    if rematch_state.host_ready && rematch_state.client_ready {
        // 调试输出已禁用: println!("[再来一局] 双方都准备好了，重新开始游戏");
        // 重置游戏状态
        commands.remove_resource::<RoundInfo>();
        commands.remove_resource::<RematchState>();
        // 重新开始游戏
        app_state.set(AppState::Playing);
        next_round_state.set(RoundState::Attacking);
    }
}

// --- 视图系统 ---

/// 更新viewport大小（只处理本地双人模式）
pub fn update_viewports(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut window_resized_reader: EventReader<WindowResized>,
    mut camera_queries: ParamSet<(
        Query<&Camera, (With<Camera2d>, With<PlayerCamera>)>,
        Query<&mut Camera, (With<Camera2d>, With<PlayerCamera>)>,
    )>,
    room_info: Res<crate::RoomInfo>,
    mut ui_state: ResMut<UiStateTracker>,
) {
    // 只在本地模式下更新视口
    if room_info.is_connected {
        return;
    }
    
    // 窗口大小改变时更新
    let mut needs_update = false;
    for window_resized in window_resized_reader.read() {
        if let Ok(window) = windows.get(window_resized.window) {
            let window_width = window.physical_width();
            let window_height = window.physical_height();
            let half_width = window_width / 2;
            let mut camera_query = camera_queries.p1();
            update_camera_viewports(&mut camera_query, half_width, window_height);
            needs_update = true;
        }
    }
    
    // 初始设置（只在未初始化时运行一次）
    if !ui_state.viewport_initialized {
        if let Ok(window) = windows.get_single() {
            let window_width = window.physical_width();
            let window_height = window.physical_height();
            
            // 只有当窗口大小有效时才更新
            if window_width > 0 && window_height > 0 {
                let half_width = window_width / 2;
                
                // 先检查游戏相机（order < 2）的数量和状态
                let read_query = camera_queries.p0();
                let cameras: Vec<_> = read_query.iter().collect();
                let game_cameras: Vec<_> = cameras.iter()
                    .filter(|cam| cam.order < 2) // 只处理order 0和1的相机（游戏相机）
                    .collect();
                
                if game_cameras.len() >= 2 {
                    // 检查是否需要更新视口
                    let needs_init = game_cameras.iter().any(|cam| {
                        cam.viewport.is_none() || 
                        cam.viewport.as_ref().map(|v| v.physical_size.x == 0 || v.physical_size.y == 0).unwrap_or(true)
                    });
                    
                    if needs_init {
                        let mut camera_query = camera_queries.p1();
                        update_camera_viewports(&mut camera_query, half_width, window_height);
                        ui_state.viewport_initialized = true;
                    }
                }
            }
        }
    }
}

/// 更新摄像机视口
pub fn update_camera_viewports(
    camera_query: &mut Query<&mut Camera, (With<Camera2d>, With<PlayerCamera>)>,
    half_width: u32,
    window_height: u32,
) {
    // 直接遍历所有相机并更新
    for mut camera in camera_query.iter_mut() {
        // 只更新order 0和1的相机（游戏相机）
        if camera.order == 0 {
            // 左侧相机（进攻方视角）
            camera.viewport = Some(Viewport {
                physical_position: UVec2::new(0, 0),
                physical_size: UVec2::new(half_width, window_height),
                depth: 0.0..0.5,
            });
        } else if camera.order == 1 {
            // 右侧相机（防守方视角）
            camera.viewport = Some(Viewport {
                physical_position: UVec2::new(half_width, 0),
                physical_size: UVec2::new(half_width, window_height),
                depth: 0.5..1.0,
            });
        }
    }
}

/// 检查图片加载状态（调试用）
pub fn check_image_loading_system(
    head_query: Query<(&Handle<Image>, &HumanoidPart), With<HumanoidPart>>,
    images: Res<Assets<Image>>,
) {
    static mut CHECKED: bool = false;
    unsafe {
        if !CHECKED {
            for (image_handle, humanoid_part) in head_query.iter() {
                if matches!(humanoid_part.part_type, HumanoidPartType::Head) {
                    if let Some(image) = images.get(image_handle) {
                        // 调试输出已禁用: println!("图片已加载: 玩家={:?}, 尺寸={:?}x{:?}", humanoid_part.player_id, image.texture_descriptor.size.width, image.texture_descriptor.size.height);
                    } else {
                        // 调试输出已禁用: println!("图片未加载: 玩家={:?}, 句柄={:?}", humanoid_part.player_id, image_handle.id());
                    }
                }
            }
            CHECKED = true;
        }
    }
}

