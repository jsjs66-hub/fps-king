use bevy::prelude::*;
use crate::AppState;
use crate::FontResource;
use crate::RoomInfo;
use crate::network_game::{NetworkManager, NetworkMessage};
use bincode;

/// 房间UI组件
#[derive(Component)]
pub struct RoomUI;

#[derive(Component)]
pub struct RoomCodeText;

#[derive(Component)]
pub struct StartGameButton;

#[derive(Component)]
pub struct BackButton;

#[derive(Component)]
pub struct PlayerStatusText;

#[derive(Component)]
pub struct PlayerCountText;

#[derive(Component)]
pub struct HostIpText;

/// 设置创建房间页面（等待页面）- 简化版（自动发现）
pub fn setup_creating_room(
    mut commands: Commands,
    font_resource: Res<FontResource>,
    room_info: Res<RoomInfo>,
    network_manager: Res<NetworkManager>,
) {
    let font = font_resource.font.clone();
    // 从网络管理器获取房间ID
    let room_id = network_manager.room_id.lock().unwrap().clone();
    let room_code = if room_id.is_empty() {
        "创建中...".to_string()
    } else {
        room_id
    };
    
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
        RoomUI,
    )).with_children(|parent| {
        // 标题
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "创建房间",
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
        
        // 房间号标签
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "房间ID",
                TextStyle {
                    font: font.clone(),
                    font_size: 32.0,
                    color: Color::WHITE,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(20.0)),
                ..default()
            },
            ..default()
        });
        
        // 房间号显示（可以动态更新）
        parent.spawn((
            TextBundle {
                text: Text::from_sections([TextSection::new(
                    room_code,
                    TextStyle {
                        font: font.clone(),
                        font_size: 48.0,
                        color: Color::YELLOW,
                    },
                )]),
                style: Style {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
                ..default()
            },
            RoomCodeText,
        ));
        
        // IP地址显示
        let ip_text = if let Some(local_ip) = network_manager.local_ip.lock().unwrap().as_ref() {
            format!("其他玩家请连接到: {}:12345", local_ip)
        } else {
            "正在获取IP地址...".to_string()
        };
        parent.spawn((
            TextBundle {
                text: Text::from_sections([TextSection::new(
                    ip_text,
                    TextStyle {
                        font: font.clone(),
                        font_size: 28.0,
                        color: Color::CYAN,
                    },
                )]),
                style: Style {
                    margin: UiRect::bottom(Val::Px(40.0)),
                    ..default()
                },
                ..default()
            },
            HostIpText,
        ));
        
        // 玩家状态显示
        parent.spawn((
            TextBundle {
                text: Text::from_sections([TextSection::new(
                    "等待其他玩家加入...",
                    TextStyle {
                        font: font.clone(),
                        font_size: 24.0,
                        color: Color::GRAY,
                    },
                )]),
                style: Style {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
                ..default()
            },
            PlayerStatusText,
        ));
        
        // 已加入玩家显示
        parent.spawn((
            TextBundle {
                text: Text::from_sections([TextSection::new(
                    "玩家: 1/2",
                    TextStyle {
                        font: font.clone(),
                        font_size: 20.0,
                        color: Color::WHITE,
                    },
                )]),
                style: Style {
                    margin: UiRect::bottom(Val::Px(40.0)),
                    ..default()
                },
                ..default()
            },
            PlayerCountText,
        ));
        
        // 开始游戏按钮（只有房主可以看到）
        parent.spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(300.0),
                    height: Val::Px(80.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
                background_color: Color::rgb(0.2, 0.8, 0.2).into(),
                ..default()
            },
            StartGameButton,
        )).with_children(|button| {
            button.spawn(TextBundle {
                text: Text::from_sections([TextSection::new(
                    "开始游戏",
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
        parent.spawn((
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
            BackButton,
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
}

/// IP输入框资源（存储用户输入的IP地址）
#[derive(Resource, Default)]
pub struct IpInputResource {
    pub ip_text: String,
    pub is_editing: bool, // 是否正在编辑
}

#[derive(Component)]
pub struct IpInputText;

#[derive(Component)]
pub struct ConnectIpButton;

#[derive(Component)]
pub struct IpInputBox; // IP输入框容器

/// 设置加入房间页面（支持手动输入IP）
pub fn setup_joining_room_simple(
    mut commands: Commands,
    font_resource: Res<FontResource>,
) {
    let font = font_resource.font.clone();
    
    // 初始化IP输入资源（默认IP地址）
    commands.insert_resource(IpInputResource {
        ip_text: "172.19.150.35:12345".to_string(),
        is_editing: false,
    });
    
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
        RoomUI,
    )).with_children(|parent| {
        // 标题
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "加入房间",
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
        
        // IP地址输入提示
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "连接到指定IP地址",
                TextStyle {
                    font: font.clone(),
                    font_size: 32.0,
                    color: Color::WHITE,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(20.0)),
                ..default()
            },
            ..default()
        });
        
        // IP输入提示文字
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "输入IP地址（格式：IP:端口，使用数字键和分号键输入，按Enter确认）",
                TextStyle {
                    font: font.clone(),
                    font_size: 18.0,
                    color: Color::GRAY,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(10.0)),
                ..default()
            },
            ..default()
        });
        
        // IP输入框容器（可点击激活编辑）
        parent.spawn((
            IpInputBox,
            ButtonBundle {
                style: Style {
                    width: Val::Px(500.0),
                    height: Val::Px(60.0),
                    margin: UiRect::bottom(Val::Px(20.0)),
                    border: UiRect::all(Val::Px(3.0)),
                    padding: UiRect::all(Val::Px(10.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::rgb(0.15, 0.15, 0.2).into(),
                border_color: Color::rgb(0.5, 0.5, 0.7).into(),
                ..default()
            },
        )).with_children(|input_box| {
            input_box.spawn((
                TextBundle {
                    text: Text::from_sections([TextSection::new(
                        "172.19.150.35:12345",
                        TextStyle {
                            font: font.clone(),
                            font_size: 28.0,
                            color: Color::WHITE,
                        },
                    )]),
                    ..default()
                },
                IpInputText,
            ));
        });
        
        // 连接按钮
        parent.spawn((
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
            ConnectIpButton,
        )).with_children(|button| {
            button.spawn(TextBundle {
                text: Text::from_sections([TextSection::new(
                    "连接",
                    TextStyle {
                        font: font.clone(),
                        font_size: 32.0,
                        color: Color::WHITE,
                    },
                )]),
                ..default()
            });
        });
        
        // 提示信息
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "提示：点击输入框激活编辑，输入IP后按Enter确认，然后点击连接按钮",
                TextStyle {
                    font: font.clone(),
                    font_size: 18.0,
                    color: Color::GRAY,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(20.0)),
                ..default()
            },
            ..default()
        });
        
        // 自动搜索提示
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "或等待自动搜索局域网房间...",
                TextStyle {
                    font: font.clone(),
                    font_size: 18.0,
                    color: Color::GRAY,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(40.0)),
                ..default()
            },
            ..default()
        });
        
        // 返回按钮
        parent.spawn((
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
            BackButton,
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
}

/// 重新连接事件
#[derive(Event)]
pub struct ReconnectEvent {
    pub ip_address: String,
}

/// 处理IP输入框点击（激活编辑模式）
pub fn handle_ip_input_box_click(
    mut interaction_query: Query<&Interaction, (Changed<Interaction>, With<IpInputBox>)>,
    mut ip_input_resource: ResMut<IpInputResource>,
) {
    for interaction in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            ip_input_resource.is_editing = true;
            // 调试输出已禁用: println!("[客户端] IP输入框已激活，可以输入IP地址");
        }
    }
}

/// 处理键盘输入（编辑IP地址）
pub fn handle_ip_keyboard_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut ip_input_resource: ResMut<IpInputResource>,
    mut ip_text_query: Query<&mut Text, With<IpInputText>>,
) {
    if !ip_input_resource.is_editing {
        return;
    }
    
    let mut text_changed = false;
    
    // 处理特殊按键（Backspace, Enter, Escape）
    if keyboard_input.just_pressed(KeyCode::Backspace) {
        if !ip_input_resource.ip_text.is_empty() {
            ip_input_resource.ip_text.pop();
            text_changed = true;
        }
    }
    
    if keyboard_input.just_pressed(KeyCode::Enter) {
        ip_input_resource.is_editing = false;
        // 调试输出已禁用: println!("[客户端] IP地址输入完成: {}", ip_input_resource.ip_text);
        text_changed = true;
    }
    
    if keyboard_input.just_pressed(KeyCode::Escape) {
        ip_input_resource.is_editing = false;
        ip_input_resource.ip_text = "172.19.150.35:12345".to_string();
        text_changed = true;
    }
    
    // 处理数字键和小数点、冒号
    for keycode in keyboard_input.get_just_pressed() {
        if let Some(ch) = keycode_to_char(*keycode) {
            ip_input_resource.ip_text.push(ch);
            text_changed = true;
        }
    }
    
    // 更新显示文本
    if text_changed {
        for mut text in ip_text_query.iter_mut() {
            text.sections[0].value = ip_input_resource.ip_text.clone();
            // 如果正在编辑，改变颜色
            if ip_input_resource.is_editing {
                text.sections[0].style.color = Color::YELLOW;
            } else {
                text.sections[0].style.color = Color::WHITE;
            }
        }
    }
}

/// 将KeyCode转换为字符（数字键、点、冒号）
fn keycode_to_char(keycode: KeyCode) -> Option<char> {
    match keycode {
        KeyCode::Digit0 => Some('0'),
        KeyCode::Digit1 => Some('1'),
        KeyCode::Digit2 => Some('2'),
        KeyCode::Digit3 => Some('3'),
        KeyCode::Digit4 => Some('4'),
        KeyCode::Digit5 => Some('5'),
        KeyCode::Digit6 => Some('6'),
        KeyCode::Digit7 => Some('7'),
        KeyCode::Digit8 => Some('8'),
        KeyCode::Digit9 => Some('9'),
        KeyCode::Numpad0 => Some('0'),
        KeyCode::Numpad1 => Some('1'),
        KeyCode::Numpad2 => Some('2'),
        KeyCode::Numpad3 => Some('3'),
        KeyCode::Numpad4 => Some('4'),
        KeyCode::Numpad5 => Some('5'),
        KeyCode::Numpad6 => Some('6'),
        KeyCode::Numpad7 => Some('7'),
        KeyCode::Numpad8 => Some('8'),
        KeyCode::Numpad9 => Some('9'),
        KeyCode::Period | KeyCode::NumpadDecimal => Some('.'),
        KeyCode::Semicolon => Some(':'), // 分号键输入冒号（IP地址格式：IP:端口）
        _ => None,
    }
}

/// 处理IP输入和连接按钮
pub fn handle_ip_input_and_connect(
    mut interaction_query: Query<(&Interaction, Entity), (Changed<Interaction>, With<Button>)>,
    connect_button_query: Query<Entity, With<ConnectIpButton>>,
    ip_input_resource: Res<IpInputResource>,
    mut reconnect_events: EventWriter<ReconnectEvent>,
) {
    for (interaction, entity) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            if let Ok(connect_entity) = connect_button_query.get_single() {
                if entity == connect_entity {
                    // 使用输入的IP地址
                    let ip_address = if ip_input_resource.ip_text.is_empty() {
                        "172.19.150.35:12345".to_string() // 默认IP
                    } else {
                        ip_input_resource.ip_text.clone()
                    };
                    // 调试输出已禁用: println!("[客户端] 用户点击连接按钮，连接到: {}", ip_address);
                    // 发送重新连接事件
                    reconnect_events.send(ReconnectEvent {
                        ip_address: ip_address.clone(),
                    });
                }
            }
        }
    }
}

/// 重新连接标记资源
#[derive(Resource, Default)]
pub struct ReconnectFlag {
    pub needs_reconnect: bool,
    pub ip_address: Option<String>,
}

/// 处理重新连接事件
pub fn handle_reconnect_event(
    mut reconnect_events: EventReader<ReconnectEvent>,
    mut network_manager: ResMut<NetworkManager>,
    mut reconnect_flag: ResMut<ReconnectFlag>,
) {
    for event in reconnect_events.read() {
        // 调试输出已禁用: println!("[客户端] 处理重新连接事件，IP地址: {}", event.ip_address);
        // 保存到网络管理器
        *network_manager.manual_ip.lock().unwrap() = Some(event.ip_address.clone());
        // 设置重新连接标志
        reconnect_flag.needs_reconnect = true;
        reconnect_flag.ip_address = Some(event.ip_address.clone());
        // 调试输出已禁用: println!("[客户端] 已设置重新连接标志，IP地址: {}", event.ip_address);
    }
}

/// 执行重新连接
pub fn execute_reconnect(
    mut network_manager: ResMut<NetworkManager>,
    mut room_info: ResMut<RoomInfo>,
    mut reconnect_flag: ResMut<ReconnectFlag>,
) {
    if reconnect_flag.needs_reconnect {
        reconnect_flag.needs_reconnect = false;
        if let Some(ip_address) = reconnect_flag.ip_address.take() {
            // 调试输出已禁用: println!("[客户端] 执行重新连接，IP地址: {}", ip_address);
            
            // 确保manual_ip已经设置
            *network_manager.manual_ip.lock().unwrap() = Some(ip_address.clone());
            
            // 清理旧的socket（如果存在）- 这会停止所有相关线程
            if let Some(old_socket) = network_manager.socket.take() {
                // 调试输出已禁用: println!("[客户端] 清理旧的socket连接");
                // 关闭socket会停止所有使用它的线程
                if let Ok(socket_guard) = old_socket.lock() {
                    // 尝试关闭socket（虽然UdpSocket没有显式的close方法，但drop会关闭它）
                    drop(socket_guard);
                }
            }
            
            // 清理远程地址和房间ID
            *network_manager.remote_addr.lock().unwrap() = None;
            *network_manager.room_id.lock().unwrap() = String::new();
            room_info.room_code = None;
            room_info.is_connected = false;
            
            // 等待一小段时间，确保旧socket完全关闭
            std::thread::sleep(std::time::Duration::from_millis(200));
            
            // 重新初始化搜索（会自动使用manual_ip）
            // 调试输出已禁用: println!("[客户端] 开始新的连接尝试...");
            crate::network_game::search_room(network_manager, room_info);
            // 调试输出已禁用: println!("[客户端] 已开始连接到: {}", ip_address);
        }
    }
}

/// 如果没有手动IP且还没有开始搜索，则自动搜索
pub fn auto_search_if_needed(
    mut network_manager: ResMut<NetworkManager>,
    mut room_info: ResMut<RoomInfo>,
    mut has_searched: Local<bool>,
) {
    // 如果已经搜索过，则跳过
    if *has_searched {
        return;
    }
    
    // 检查是否有手动IP
    let has_manual_ip = network_manager.manual_ip.lock().unwrap().is_some();
    if has_manual_ip {
        // 有手动IP，等待用户点击连接按钮
        return;
    }
    
    // 检查是否已经有socket（说明已经开始搜索）
    if network_manager.socket.is_some() {
        *has_searched = true;
        return;
    }
    
    // 延迟一小段时间后自动搜索（给用户时间输入IP）
    *has_searched = true;
    // 调试输出已禁用: println!("[客户端] 开始自动搜索房间...");
    crate::network_game::search_room(network_manager, room_info);
}

/// 更新IP地址显示（如果IP地址在创建房间后才获取到）
pub fn update_host_ip_display(
    network_manager: Res<NetworkManager>,
    mut ip_text_query: Query<&mut Text, (With<HostIpText>, Without<RoomCodeText>)>,
) {
    if let Some(local_ip) = network_manager.local_ip.lock().unwrap().as_ref() {
        let ip_text = format!("其他玩家请连接到: {}:12345", local_ip);
        for mut text in ip_text_query.iter_mut() {
            if text.sections.len() > 0 {
                text.sections[0].value = ip_text.clone();
            }
        }
    }
}

/// 更新房间号显示
pub fn update_room_code_display(
    network_manager: Res<NetworkManager>,
    mut room_code_text_query: Query<&mut Text, With<RoomCodeText>>,
) {
    let room_id = network_manager.room_id.lock().unwrap().clone();
    if !room_id.is_empty() {
        for mut text in room_code_text_query.iter_mut() {
            text.sections[0].value = room_id.clone();
        }
    }
}

/// 更新创建房间页面的状态显示（显示客户端是否已加入）
pub fn update_creating_room_status(
    network_manager: Res<NetworkManager>,
    mut queries: ParamSet<(
        Query<&mut Text, With<PlayerStatusText>>,
        Query<&mut Text, With<PlayerCountText>>,
        Query<&mut BackgroundColor, With<StartGameButton>>,
    )>,
) {
    // 检查是否有客户端已连接（通过remote_addr判断）
    let has_client = network_manager.remote_addr.lock().unwrap().is_some();
    
    // 更新玩家状态文本
    for mut text in queries.p0().iter_mut() {
        if has_client {
            text.sections[0].value = "玩家已加入，可以开始游戏！".to_string();
            text.sections[0].style.color = Color::GREEN;
        } else {
            text.sections[0].value = "等待其他玩家加入...".to_string();
            text.sections[0].style.color = Color::GRAY;
        }
    }
    
    // 更新玩家数量
    for mut text in queries.p1().iter_mut() {
        let count = if has_client { 2 } else { 1 };
        text.sections[0].value = format!("玩家: {}/2", count);
    }
    
    // 更新开始游戏按钮状态（只有客户端加入后才能点击）
    for mut bg_color in queries.p2().iter_mut() {
        if has_client {
            *bg_color = Color::rgb(0.2, 0.8, 0.2).into(); // 绿色，可以开始
        } else {
            *bg_color = Color::rgb(0.5, 0.5, 0.5).into(); // 灰色，不可点击
        }
    }
}

/// 处理房间内按钮点击（创建房间时）
pub fn handle_room_buttons_creating(
    mut interaction_query: Query<(&Interaction, Entity), (Changed<Interaction>, With<Button>)>,
    start_game_query: Query<Entity, With<StartGameButton>>,
    back_button_query: Query<Entity, With<BackButton>>,
    mut network_manager: ResMut<NetworkManager>,
    mut app_state: ResMut<NextState<AppState>>,
    mut room_info: ResMut<RoomInfo>,
) {
    for (interaction, entity) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            // 调试输出已禁用: println!("[房间] 检测到按钮点击: entity={:?}", entity);
            
            // 先检查是否是返回按钮
            if let Ok(back_entity) = back_button_query.get_single() {
                if entity == back_entity {
                    // 调试输出已禁用: println!("[房间] 点击了返回按钮，准备返回网络菜单");
                    room_info.is_connected = false;
                    // 清理网络资源（注意：这会移动network_manager，所以必须最后调用）
                    crate::network_game::cleanup_network(network_manager);
                    // 调试输出已禁用: println!("[房间] 已清理网络资源");
                    app_state.set(AppState::NetworkMenu);
                    return; // 立即返回，不再处理其他按钮
                }
            }
            
            // 检查是否是开始游戏按钮
            if let Ok(start_game_entity) = start_game_query.get_single() {
                if entity == start_game_entity {
                    // 调试输出已禁用: println!("[房间] 点击了开始游戏按钮");
                    if room_info.is_host {
                        // 检查是否有客户端已连接
                        let remote_addr = network_manager.remote_addr.lock().unwrap();
                        let has_client = remote_addr.is_some();
                        
                        // 调试输出已禁用: println!("[房主] 检查客户端连接状态: has_client={}, remote_addr={:?}", has_client, *remote_addr);
                        
                        // 允许房主在没有客户端时也能开始游戏（用于测试）
                        // 如果有客户端，则发送开始游戏消息
                        if has_client {
                            // 房主点击开始游戏
                            room_info.is_connected = true;
                            // 调试输出已禁用: println!("[房主] 开始游戏，remote_addr: {:?}, socket: {:?}", *remote_addr, if network_manager.socket.is_some() { "已设置" } else { "未设置" });
                            // 发送开始游戏消息给客户端（发送多次以确保客户端收到）
                            drop(remote_addr); // 释放锁
                            for i in 0..3 {
                                crate::network_game::send_network_message(&*network_manager, NetworkMessage::StartGame);
                                // 调试输出已禁用: println!("[房主] 已发送第 {} 次 StartGame 消息", i + 1);
                            }
                            // 调试输出已禁用: println!("[房主] StartGame消息已发送，切换到Playing状态");
                        } else {
                            // 没有客户端，但允许房主开始游戏（用于测试）
                            // 调试输出已禁用: println!("[房主] 警告：没有客户端连接，但允许开始游戏（测试模式）");
                            room_info.is_connected = true;
                        }
                        
                        // 切换到Playing状态
                        app_state.set(AppState::Playing);
                        // 调试输出已禁用: println!("[房主] 状态已切换到Playing");
                    } else {
                        // 调试输出已禁用: println!("[房间] 错误：非房主点击了开始游戏按钮");
                    }
                }
            } else {
                // 调试输出已禁用: println!("[房间] 警告：找不到开始游戏按钮实体");
            }
        }
    }
}

/// 设置客户端在房间内等待的UI
pub fn setup_in_room(
    mut commands: Commands,
    font_resource: Res<FontResource>,
    network_manager: Res<NetworkManager>,
    room_info: Res<RoomInfo>,
) {
    let font = font_resource.font.clone();
    let room_id = network_manager.room_id.lock().unwrap().clone();
    
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
        RoomUI,
    )).with_children(|parent| {
        // 标题
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "在房间中",
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
        
        // 房间ID显示
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                format!("房间ID: {}", room_id),
                TextStyle {
                    font: font.clone(),
                    font_size: 32.0,
                    color: Color::YELLOW,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(40.0)),
                ..default()
            },
            ..default()
        });
        
        // 房主信息（显示为"房主已就绪"）
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "房主: 已就绪",
                TextStyle {
                    font: font.clone(),
                    font_size: 28.0,
                    color: Color::GREEN,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(20.0)),
                ..default()
            },
            ..default()
        });
        
        // 等待提示
        parent.spawn(TextBundle {
            text: Text::from_sections([TextSection::new(
                "等待房主开始游戏...",
                TextStyle {
                    font: font.clone(),
                    font_size: 24.0,
                    color: Color::GRAY,
                },
            )]),
            style: Style {
                margin: UiRect::bottom(Val::Px(60.0)),
                ..default()
            },
            ..default()
        });
        
        // 返回按钮
        parent.spawn((
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
            BackButton,
        )).with_children(|button| {
            button.spawn(TextBundle {
                text: Text::from_sections([TextSection::new(
                    "离开房间",
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
}

/// 处理房间内按钮点击（在房间内等待时）
pub fn handle_room_buttons_in_room(
    interaction_query: Query<(&Interaction, Entity), (Changed<Interaction>, With<Button>)>,
    back_button_query: Query<Entity, With<BackButton>>,
    mut app_state: ResMut<NextState<AppState>>,
    mut room_info: ResMut<RoomInfo>,
) {
    for (interaction, entity) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            // 检查是否是返回按钮
            if let Ok(back_entity) = back_button_query.get_single() {
                if entity == back_entity {
                    room_info.is_connected = false;
                    app_state.set(AppState::NetworkMenu);
                }
            }
        }
    }
}

/// 清理房间UI
pub fn cleanup_room_ui(
    mut commands: Commands,
    query: Query<Entity, With<RoomUI>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    // 调试输出已禁用: println!("[清理] 房间UI已清理");
}
