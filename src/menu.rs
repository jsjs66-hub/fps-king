use bevy::prelude::*;
use crate::AppState;
use crate::FontResource;

/// 主菜单UI组件
#[derive(Component)]
pub struct MainMenuButton;

/// 主菜单根节点标记
#[derive(Component)]
pub struct MainMenuUI;

#[derive(Component)]
pub enum MenuButtonType {
    LocalMultiplayer,
    NetworkMatch,
    Settings,
}

/// 设置主菜单
pub fn setup_main_menu(
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
        MainMenuUI,
    )).with_children(|parent| {
        // 标题
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "我是赋能哥",
                TextStyle {
                    font: font.clone(),
                    font_size: 72.0,
                    color: Color::WHITE,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(80.0)),
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
            // 本地双人按钮
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
                    background_color: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..default()
                },
                MainMenuButton,
                MenuButtonType::LocalMultiplayer,
            )).with_children(|button| {
                button.spawn(TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "本地双人",
                        TextStyle {
                            font: font.clone(),
                            font_size: 32.0,
                            color: Color::WHITE,
                        },
                    )]),
                    ..default()
                });
            });
            
            // 局域网对战按钮
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
                    background_color: Color::rgb(0.8, 0.4, 0.2).into(),
                    ..default()
                },
                MainMenuButton,
                MenuButtonType::NetworkMatch,
            )).with_children(|button| {
                button.spawn(TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "局域网对战",
                        TextStyle {
                            font: font.clone(),
                            font_size: 32.0,
                            color: Color::WHITE,
                        },
                    )]),
                    ..default()
                });
            });
            
            // 设置按钮
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
                MainMenuButton,
                MenuButtonType::Settings,
            )).with_children(|button| {
                button.spawn(TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "设置",
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

/// 处理主菜单按钮点击
pub fn handle_main_menu_buttons(
    mut interaction_query: Query<
        (&Interaction, &MenuButtonType),
        (Changed<Interaction>, With<Button>),
    >,
    mut app_state: ResMut<NextState<AppState>>,
) {
    for (interaction, button_type) in interaction_query.iter_mut() {
        if *interaction == Interaction::Pressed {
            match button_type {
                MenuButtonType::LocalMultiplayer => {
                    app_state.set(AppState::LocalMultiplayer);
                }
                MenuButtonType::NetworkMatch => {
                    app_state.set(AppState::NetworkMenu);
                }
                MenuButtonType::Settings => {
                    // 设置功能暂未实现
                    // 调试输出已禁用: println!("设置功能暂未实现");
                }
            }
        }
    }
}

/// 清理主菜单
pub fn cleanup_main_menu(
    mut commands: Commands,
    query: Query<Entity, With<MainMenuUI>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    // 调试输出已禁用: println!("[清理] 主菜单UI已清理");
}

