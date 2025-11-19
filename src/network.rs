use bevy::prelude::*;
use crate::AppState;
use crate::FontResource;

/// 网络房间信息
#[derive(Resource, Default)]
pub struct RoomInfo {
    pub room_code: Option<String>,
    pub is_host: bool,
    pub is_connected: bool,
}

/// 网络菜单UI组件
#[derive(Component)]
pub struct NetworkMenuButton;

/// 网络菜单根节点标记
#[derive(Component)]
pub struct NetworkMenuUI;

#[derive(Component)]
pub enum NetworkButtonType {
    CreateRoom,
    JoinRoom,
    Back,
}

/// 设置网络菜单
pub fn setup_network_menu(
    mut commands: Commands,
    font_resource: Res<FontResource>,
) {
    let font = font_resource.font.clone();
    
    // 背景
    commands.spawn((
        NodeBundle {
        style: Style {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        background_color: Color::rgba(0.1, 0.1, 0.15, 1.0).into(),
        ..default()
        },
        NetworkMenuUI,
    )).with_children(|parent| {
        // 标题
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "局域网对战",
                TextStyle {
                    font: font.clone(),
                    font_size: 64.0,
                    color: Color::WHITE,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(60.0)),
                ..default()
            },
            ..default()
        });
        
        // 按钮容器
        parent.spawn(NodeBundle {
            style: Style {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            ..default()
        }).with_children(|buttons| {
            // 创建房间按钮
            buttons.spawn((
                ButtonBundle {
                    style: Style {
                        width: Val::Px(300.0),
                        height: Val::Px(80.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        margin: UiRect::bottom(Val::Px(20.0)),
                        ..default()
                    },
                    background_color: Color::rgb(0.2, 0.6, 0.2).into(),
                    ..default()
                },
                NetworkMenuButton,
                NetworkButtonType::CreateRoom,
            )).with_children(|button| {
                button.spawn(TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "创建房间",
                        TextStyle {
                            font: font.clone(),
                            font_size: 32.0,
                            color: Color::WHITE,
                        },
                    )]),
                    ..default()
                });
            });
            
            // 加入房间按钮
            buttons.spawn((
                ButtonBundle {
                    style: Style {
                        width: Val::Px(300.0),
                        height: Val::Px(80.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        margin: UiRect::bottom(Val::Px(20.0)),
                        ..default()
                    },
                    background_color: Color::rgb(0.6, 0.4, 0.2).into(),
                    ..default()
                },
                NetworkMenuButton,
                NetworkButtonType::JoinRoom,
            )).with_children(|button| {
                button.spawn(TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "加入房间",
                        TextStyle {
                            font: font.clone(),
                            font_size: 32.0,
                            color: Color::WHITE,
                        },
                    )]),
                    ..default()
                });
            });
            
            // 返回按钮
            buttons.spawn((
                ButtonBundle {
                    style: Style {
                        width: Val::Px(300.0),
                        height: Val::Px(80.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::rgb(0.5, 0.5, 0.5).into(),
                    ..default()
                },
                NetworkMenuButton,
                NetworkButtonType::Back,
            )).with_children(|button| {
                button.spawn(TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "返回",
                        TextStyle {
                            font: font.clone(),
                            font_size: 32.0,
                            color: Color::WHITE,
                        },
                    )]),
                    ..default()
                });
            });
        });
    });
}

/// 处理网络菜单按钮点击
pub fn handle_network_menu_buttons(
    mut interaction_query: Query<
        (&Interaction, &NetworkButtonType),
        (Changed<Interaction>, With<Button>),
    >,
    mut app_state: ResMut<NextState<AppState>>,
    _room_info: Res<RoomInfo>,
) {
    for (interaction, button_type) in interaction_query.iter_mut() {
        if *interaction == Interaction::Pressed {
            match button_type {
                NetworkButtonType::CreateRoom => {
                    // 创建房间并自动开始搜索
                    app_state.set(AppState::CreatingRoom);
                }
                NetworkButtonType::JoinRoom => {
                    // 自动搜索并加入房间
                    app_state.set(AppState::JoiningRoom);
                }
                NetworkButtonType::Back => {
                    app_state.set(AppState::MainMenu);
                }
            }
        }
    }
}

/// 生成4位房间号
fn generate_room_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:04}", rng.gen_range(1000..10000))
}

/// 清理网络菜单
pub fn cleanup_network_menu(
    mut commands: Commands,
    query: Query<Entity, With<NetworkMenuUI>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    // 调试输出已禁用: println!("[清理] 网络菜单UI已清理");
}

