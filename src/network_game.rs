use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy::ecs::system::ParamSet;
use std::net::{UdpSocket, SocketAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::io;
use serde::{Serialize, Deserialize};
use rand::Rng;
use crate::{AppState, RoomInfo};
use crate::PlayerId;
use crate::PlayerRole;

/// 网络消息类型
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NetworkMessage {
    // 房间发现
    RoomDiscoveryRequest,  // 请求发现房间
    RoomDiscoveryResponse { room_id: String, player_name: String },  // 响应房间发现
    
    // 连接
    JoinRequest { room_id: String },  // 请求加入房间
    JoinAccept { player_id: PlayerId },  // 接受加入
    JoinReject,  // 拒绝加入
    
    // 游戏状态同步
    GameState { 
        player_positions: Vec<(PlayerId, [f32; 3])>,  // Vec3 序列化为 [f32; 3]
        player_roles: Vec<(PlayerId, PlayerRole)>,
        health: Vec<(PlayerId, f32)>,
    },
    
    // 玩家输入
    PlayerInput {
        player_id: PlayerId,
        movement: Option<[f32; 2]>,  // Vec2 序列化为 [f32; 2]
        action: Option<String>,
        crosshair_pos: Option<[f32; 2]>,  // Vec2 序列化为 [f32; 2]
    },
    
    // 游戏事件
    PlayerHit { player_id: PlayerId, damage: f32 },
    GameOver { winner: PlayerId },
    StartGame,
    
    // 回合信息同步
    RoundInfoSync {
        current_attacker: PlayerId,
        bullets_left: u32,
        round_timer_remaining: f32,
        p1_health: f32,
        p2_health: f32,
        bullets_fired: u32,
        bullets_hit: u32,
    },
    
    // 角色切换
    SwitchRoles {
        new_attacker: PlayerId,
    },
    
    // 准星位置同步（进攻方发送给防守方）
    CrosshairPosition { position: [f32; 2] },
    
    // 防守方位置和动作同步（防守方发送给进攻方）
    DefenderState {
        position: [f32; 3],
        dodge_action: String, // "None", "Crouch", "SideLeft", "SideRight"
    },
    
    // 子弹同步（发射子弹时发送）
    BulletSpawn {
        bullet_id: u64,  // 子弹同步ID
        owner: PlayerId,  // 发射者
        start_pos: [f32; 2],  // 起始位置
        target_pos: [f32; 2],  // 目标位置
        velocity: [f32; 2],  // 速度
    },
    
    // 血量更新（被击中时发送）
    HealthUpdate {
        player_id: PlayerId,
        health: f32,
    },
    
    // 再来一局
    RematchRequest,  // 请求再来一局
    RematchReady,    // 准备再来一局（双方都点击后）
}

/// 网络管理器资源
#[derive(Resource)]
pub struct NetworkManager {
    pub socket: Option<Arc<Mutex<UdpSocket>>>,
    pub is_host: bool,
    pub remote_addr: Arc<Mutex<Option<SocketAddr>>>,
    pub room_id: Arc<Mutex<String>>,
    pub message_queue: Arc<Mutex<Vec<NetworkMessage>>>,
    pub local_ip: Arc<Mutex<Option<Ipv4Addr>>>,  // 本地IP地址
    pub manual_ip: Arc<Mutex<Option<String>>>,  // 手动输入的IP地址
    pub is_running: Arc<AtomicBool>,            // 网络线程运行标志
}

// 确保 NetworkManager 是 Send + Sync
unsafe impl Send for NetworkManager {}
unsafe impl Sync for NetworkManager {}

impl Default for NetworkManager {
    fn default() -> Self {
        Self {
            socket: None,
            is_host: false,
            remote_addr: Arc::new(Mutex::new(None)),
            room_id: Arc::new(Mutex::new(String::new())),
            message_queue: Arc::new(Mutex::new(Vec::new())),
            local_ip: Arc::new(Mutex::new(None)),
            manual_ip: Arc::new(Mutex::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// 获取Windows主机IP地址（在WSL2环境中）
fn get_windows_host_ip() -> Option<Ipv4Addr> {
    // 方法1: 通过 /etc/resolv.conf 获取（WSL2会在这里写入Windows主机的IP）
    if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in content.lines() {
            if line.starts_with("nameserver ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(ip) = parts[1].parse::<Ipv4Addr>() {
                        if !ip.is_loopback() {
                            // 调试输出已禁用: println!("[网络] 发现Windows主机IP（通过resolv.conf）: {}", ip);
                            return Some(ip);
                        }
                    }
                }
            }
        }
    }
    
    // 方法2: 通过ip route命令获取默认网关（通常是Windows主机）
    if let Ok(output) = std::process::Command::new("ip")
        .args(&["route", "show", "default"])
        .output()
    {
        let output_str = String::from_utf8_lossy(&output.stdout);
        for line in output_str.lines() {
            if line.contains("default via ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for (i, part) in parts.iter().enumerate() {
                    if *part == "via" && i + 1 < parts.len() {
                        if let Ok(ip) = parts[i + 1].parse::<Ipv4Addr>() {
                            if !ip.is_loopback() && !ip.is_unspecified() {
                                // 调试输出已禁用: println!("[网络] 发现Windows主机IP（通过ip route）: {}", ip);
                                return Some(ip);
                            }
                        }
                    }
                }
            }
        }
    }
    
    None
}

/// 检查是否在WSL2环境中
fn is_wsl2() -> bool {
    // 检查是否存在 /proc/version，并且包含Microsoft或WSL
    if let Ok(content) = std::fs::read_to_string("/proc/version") {
        let content_lower = content.to_lowercase();
        if content_lower.contains("microsoft") || content_lower.contains("wsl") {
            return true;
        }
    }
    
    // 检查是否存在 /proc/sys/kernel/osrelease 且包含microsoft
    if let Ok(content) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        let content_lower = content.to_lowercase();
        if content_lower.contains("microsoft") || content_lower.contains("wsl") {
            return true;
        }
    }
    
    false
}

/// 获取ZeroTier IP地址（如果存在）
fn get_zerotier_ip() -> Option<Ipv4Addr> {
    // ZeroTier接口通常以zt开头，例如zt4homms2q
    if let Ok(output) = std::process::Command::new("ip")
        .args(&["addr", "show"])
        .output()
    {
        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut in_zt_interface = false;
        
        for line in output_str.lines() {
            // 检查是否是ZeroTier接口（以zt开头）
            if line.contains(": zt") || line.starts_with("zt") {
                in_zt_interface = true;
                continue;
            }
            
            // 如果在ZeroTier接口中，查找IP地址
            if in_zt_interface {
                if line.contains("inet ") && !line.contains("127.0.0.1") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let ip_part = parts[1];
                        if let Some(ip_str) = ip_part.split('/').next() {
                            if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
                                // ZeroTier IP通常在10.147.x.x或10.147.20.x范围内
                                if !ip.is_loopback() && !ip.is_link_local() {
                                    // 调试输出已禁用: println!("[网络] 发现ZeroTier IP: {}", ip);
                                    return Some(ip);
                                }
                            }
                        }
                    }
                }
                // 如果遇到新的接口行，重置标志
                if line.contains(": ") && !line.contains(": zt") {
                    in_zt_interface = false;
                }
            }
        }
    }
    
    None
}

/// 获取本地网络接口的IP地址（用于WSL2环境）
fn get_local_ip_addresses() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    
    // 优先检测ZeroTier IP（如果使用ZeroTier）
    if let Some(zt_ip) = get_zerotier_ip() {
        ips.push(zt_ip);
        // 调试输出已禁用: println!("[网络] 优先使用ZeroTier IP: {}", zt_ip);
    }
    
    // 如果是在WSL2环境中，尝试获取Windows主机IP（作为备选）
    if is_wsl2() {
        if let Some(windows_ip) = get_windows_host_ip() {
            if !ips.contains(&windows_ip) {
                // 调试输出已禁用: println!("[网络] 检测到WSL2环境，Windows主机IP: {}", windows_ip);
                // 调试输出已禁用: println!("[网络] 提示: 如果使用ZeroTier，请使用ZeroTier IP连接");
                // Windows主机IP作为备选
            }
        }
    }
    
    // 方法1: 通过连接到外部地址来获取本地IP（最可靠的方法）
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        // 尝试连接到一个外部地址（不需要真正连接成功）
        let _ = socket.connect("8.8.8.8:80");
        if let Ok(addr) = socket.local_addr() {
            if let SocketAddr::V4(addr_v4) = addr {
                let ip = *addr_v4.ip();
                if !ip.is_loopback() {
                    ips.push(ip);
                    // 调试输出已禁用: println!("[网络] 发现本地IP（通过连接检测）: {}", ip);
                }
            }
        }
    }
    
    // 方法2: 读取/proc/net/route获取网络接口信息（Linux/WSL）
    if let Ok(content) = std::fs::read_to_string("/proc/net/route") {
        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let interface = parts[0];
                // 获取该接口的IP地址
                if let Ok(addrs) = get_interface_ip(interface) {
                    for addr in addrs {
                        if !addr.is_loopback() && !addr.is_link_local() {
                            if !ips.contains(&addr) {
                                ips.push(addr);
                                // 调试输出已禁用: println!("[网络] 发现网络接口 {} 的IP: {}", interface, addr);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // 如果还没有找到IP，尝试通过ip命令
    if ips.is_empty() {
        if let Ok(output) = std::process::Command::new("ip")
            .args(&["addr", "show"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.contains("inet ") && !line.contains("127.0.0.1") && !line.contains("::1") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let ip_part = parts[1];
                        if let Some(ip_str) = ip_part.split('/').next() {
                            if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
                                if !ip.is_loopback() && !ip.is_link_local() {
                                    if !ips.contains(&ip) {
                                        ips.push(ip);
                                        // 调试输出已禁用: println!("[网络] 通过ip命令发现IP: {}", ip);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    ips
}

/// 获取网络接口的IP地址
fn get_interface_ip(interface: &str) -> Result<Vec<Ipv4Addr>, io::Error> {
    let mut ips = Vec::new();
    
    // 读取/proc/net/if_inet6或使用ifconfig/ip命令的输出
    // 这里简化处理，直接尝试读取系统信息
    
    // 尝试通过ip命令获取（如果可用）
    if let Ok(output) = std::process::Command::new("ip")
        .args(&["addr", "show", interface])
        .output()
    {
        let output_str = String::from_utf8_lossy(&output.stdout);
        for line in output_str.lines() {
            if line.contains("inet ") && !line.contains("127.0.0.1") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let ip_part = parts[1];
                    if let Some(ip_str) = ip_part.split('/').next() {
                        if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
                            if !ip.is_loopback() {
                                ips.push(ip);
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(ips)
}

/// 计算子网中的所有可能IP地址
fn get_subnet_ips(base_ip: Ipv4Addr, prefix_len: u8) -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    let octets = base_ip.octets();
    
    // 简化处理：对于/20子网（常见WSL2），扫描最后8位
    if prefix_len == 20 {
        // 计算网络地址（前20位保持不变，后12位清零）
        let base = ((u32::from(octets[0]) << 24)
                  | (u32::from(octets[1]) << 16)
                  | (u32::from(octets[2]) << 8)
                  | u32::from(octets[3])) >> (32 - prefix_len) << (32 - prefix_len);
        
        // 扫描.1到.254（跳过.0网络地址和.255广播地址）
        for host in 1..=254 {
            let ip_value = base | host;
            let ip = Ipv4Addr::from([
                ((ip_value >> 24) & 0xFF) as u8,
                ((ip_value >> 16) & 0xFF) as u8,
                ((ip_value >> 8) & 0xFF) as u8,
                (ip_value & 0xFF) as u8,
            ]);
            ips.push(ip);
        }
    }
    
    ips
}

/// 创建房间（作为主机）
pub fn create_room(
    mut network_manager: ResMut<NetworkManager>,
    mut room_info: ResMut<RoomInfo>,
) {
    // 生成房间ID
    let room_id = format!("ROOM_{}", rand::random::<u32>());
    *network_manager.room_id.lock().unwrap() = room_id.clone();
    network_manager.is_host = true;
    room_info.room_code = Some(room_id.clone());
    room_info.is_host = true;
    
    // 获取本地IP
    let local_ips = get_local_ip_addresses();
    let is_wsl = is_wsl2();
    
    // 优先使用ZeroTier IP，如果没有则使用其他IP
    let display_ip = local_ips.first().copied();
    
    if let Some(local_ip) = display_ip {
        *network_manager.local_ip.lock().unwrap() = Some(local_ip);
        
        // 检查是否是ZeroTier IP（通常在10.147.x.x范围内）
        let is_zerotier = local_ip.octets()[0] == 10 && local_ip.octets()[1] == 147;
        
        if is_zerotier {
            // 调试输出已禁用: println!("[主机] ZeroTier IP地址: {}", local_ip);
            // 调试输出已禁用: println!("[主机] 提示: 其他玩家应连接到: {}:12345", local_ip);
            // 调试输出已禁用: println!("[主机] 提示: 确保客户端已安装ZeroTier并加入同一网络");
        } else if is_wsl {
            // 调试输出已禁用: println!("[主机] Windows主机IP地址: {} (WSL2环境)", local_ip);
            // 调试输出已禁用: println!("[主机] 提示: 其他玩家应连接到: {}:12345", local_ip);
            // 调试输出已禁用: println!("[主机] 提示: 如果无法连接，建议使用ZeroTier虚拟局域网");
            // 调试输出已禁用: println!("[主机] 参考: ZeroTier安装指南.md");
        } else {
            // 调试输出已禁用: println!("[主机] 本地IP地址: {}", local_ip);
            // 调试输出已禁用: println!("[主机] 提示: 其他玩家应连接到: {}:12345", local_ip);
        }
    }
    
    // 创建UDP socket监听
    let socket = match UdpSocket::bind("0.0.0.0:12345") {
        Ok(s) => {
            // 调试输出已禁用: println!("[主机] UDP socket绑定成功: 0.0.0.0:12345");
            s
        }
        Err(e) => {
            eprintln!("[主机] ========================================");
            eprintln!("[主机] UDP socket绑定失败: {}", e);
            eprintln!("[主机] 错误：端口12345已被占用");
            eprintln!("[主机] ========================================");
            eprintln!("[主机] 可能的原因：");
            eprintln!("[主机]   1. 之前的游戏实例仍在运行");
            eprintln!("[主机]   2. 其他程序正在使用该端口");
            eprintln!("[主机] ========================================");
            eprintln!("[主机] 解决方法：");
            eprintln!("[主机]   1. 关闭之前的游戏实例（按 Ctrl+C 或关闭窗口）");
            eprintln!("[主机]   2. 等待几秒后重试");
            eprintln!("[主机]   3. 检查端口占用：");
            eprintln!("[主机]      Linux/Mac: lsof -i :12345 或 netstat -an | grep 12345");
            eprintln!("[主机]      Windows: netstat -ano | findstr :12345");
            eprintln!("[主机] ========================================");
            // 重置状态，避免状态不一致
            room_info.room_code = None;
            room_info.is_host = false;
            network_manager.is_host = false;
            *network_manager.room_id.lock().unwrap() = String::new();
            // 不panic，让用户看到错误信息后可以返回菜单
            return;
        }
    };
    socket.set_broadcast(true).expect("无法启用广播");
    socket.set_nonblocking(true).expect("无法设置非阻塞模式");
    // 调试输出已禁用: println!("[主机] UDP socket配置完成: broadcast=true, nonblocking=true");
    
    // 打印本地地址信息（用于调试）
    if let Ok(local_addr) = socket.local_addr() {
        // 调试输出已禁用: println!("[主机] 绑定地址: {}", local_addr);
    }
    
    let socket_arc = Arc::new(Mutex::new(socket));
    // 如果之前有socket线程，需要先停止
    network_manager.is_running.store(false, Ordering::Relaxed);
    network_manager.socket = Some(socket_arc.clone());
    network_manager.is_running.store(true, Ordering::Relaxed);
    let is_running_flag = network_manager.is_running.clone();
    
    // 调试输出已禁用: println!("房间已创建，房间ID: {}", room_id);
    // 调试输出已禁用: println!("等待其他玩家加入...");
    if let Some(local_ip) = display_ip {
        // 检查是否是ZeroTier IP
        let is_zerotier = local_ip.octets()[0] == 10 && local_ip.octets()[1] == 147;
        
        if is_zerotier {
            // 调试输出已禁用: println!("[主机] ========================================");
            // 调试输出已禁用: println!("[主机] ZeroTier网络提示:");
            // 调试输出已禁用: println!("[主机] 其他玩家应连接到: {}:12345", local_ip);
            // 调试输出已禁用: println!("[主机] 确保客户端已安装ZeroTier并加入同一网络");
            // 调试输出已禁用: println!("[主机] 网络ID: bb720a5aaecc11fe");
            // 调试输出已禁用: println!("[主机] ========================================");
        } else if is_wsl {
            // 调试输出已禁用: println!("[主机] ========================================");
            // 调试输出已禁用: println!("[主机] WSL2环境提示:");
            // 调试输出已禁用: println!("[主机] 其他玩家应连接到: {}:12345", local_ip);
            // 调试输出已禁用: println!("[主机] 如果无法连接，建议使用ZeroTier虚拟局域网");
            // 调试输出已禁用: println!("[主机] 参考: ZeroTier安装指南.md");
            // 调试输出已禁用: println!("[主机] ========================================");
        } else {
            // 调试输出已禁用: println!("[主机] 提示：如果客户端无法自动发现，请让客户端直接连接到: {}:12345", local_ip);
        }
    }
    
    // 启动接收线程
    let message_queue = network_manager.message_queue.clone();
    let room_id_arc = network_manager.room_id.clone();
    let remote_addr_for_thread = network_manager.remote_addr.clone();
    
    thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            if !is_running_flag.load(Ordering::Relaxed) {
                // 调试输出已禁用: println!("[主机] 接收线程收到停止信号，退出");
                break;
            }
            let socket_guard = match socket_arc.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    // 调试输出已禁用: println!("[主机] Socket锁定失败（可能已关闭），退出接收线程");
                    break;
                }
            };
            let recv_result = socket_guard.recv_from(&mut buf);
            drop(socket_guard);
            match recv_result {
                    Ok((size, addr)) => {
                        // println!("[主机] 收到来自 {} 的数据，大小: {} 字节", addr, size); // 已禁用：日志太多
                    // 立即保存远程地址（用于后续通信）
                    *remote_addr_for_thread.lock().unwrap() = Some(addr);
                        
                        if let Ok(msg) = bincode::deserialize::<NetworkMessage>(&buf[..size]) {
                            // println!("[主机] 收到消息: {:?}", msg); // 已禁用：日志太多
                            match msg {
                                NetworkMessage::RoomDiscoveryRequest => {
                                    // 响应房间发现请求
                                    let current_room_id = room_id_arc.lock().unwrap().clone();
                                    // println!("[主机] 收到房间发现请求，响应房间ID: {}", current_room_id); // 已禁用：日志太多
                                    let response = NetworkMessage::RoomDiscoveryResponse {
                                        room_id: current_room_id.clone(),
                                        player_name: "Host".to_string(),
                                    };
                                    if let Ok(data) = bincode::serialize(&response) {
                                        if let Ok(socket_guard) = socket_arc.lock() {
                                            match socket_guard.send_to(&data, addr) {
                                                Ok(_) => {}, // 调试输出已禁用: println!("[主机] 已发送房间发现响应到 {}，发送了 {} 字节", addr, sent),
                                                Err(e) => eprintln!("[主机] 发送房间发现响应失败: {}", e),
                                            }
                                        }
                                    } else {
                                        eprintln!("[主机] 序列化房间发现响应失败");
                                    }
                                }
                                NetworkMessage::JoinRequest { room_id: req_room_id } => {
                                    let current_room_id = room_id_arc.lock().unwrap().clone();
                                    // 调试输出已禁用: println!("[主机] 收到加入请求，请求房间ID: {}，当前房间ID: {}", req_room_id, current_room_id);
                                    if req_room_id == current_room_id {
                                        // 保存远程地址
                                        *remote_addr_for_thread.lock().unwrap() = Some(addr);
                                        // 调试输出已禁用: println!("[主机] 已保存远程地址: {}", addr);
                                        
                                        // 接受加入请求
                                        // 调试输出已禁用: println!("[主机] 房间ID匹配，接受加入请求");
                                        let accept = NetworkMessage::JoinAccept {
                                            player_id: crate::PlayerId::Player2,
                                        };
                                        if let Ok(data) = bincode::serialize(&accept) {
                                            if let Ok(socket_guard) = socket_arc.lock() {
                                                match socket_guard.send_to(&data, addr) {
                                                    Ok(_) => {}, // 调试输出已禁用: println!("[主机] 已发送加入接受消息到 {}，发送了 {} 字节", addr, sent),
                                                    Err(e) => eprintln!("[主机] 发送加入接受消息失败: {}", e),
                                                }
                                            }
                                            // 保存消息到队列
                                            if let Ok(mut queue) = message_queue.lock() {
                                                queue.push(NetworkMessage::JoinAccept {
                                                    player_id: crate::PlayerId::Player2,
                                                });
                                                // 调试输出已禁用: println!("[主机] 已将加入接受消息加入队列");
                                            }
                                        } else {
                                            eprintln!("[主机] 序列化加入接受消息失败");
                                        }
                                    } else {
                                        // 调试输出已禁用: println!("[主机] 房间ID不匹配，拒绝加入请求");
                                    }
                                }
                                _ => {
                                // 其他消息，保存到队列（远程地址已在收到消息时保存）
                                // println!("[主机] 收到消息: {:?}", msg); // 已禁用：日志太多
                                    if let Ok(mut queue) = message_queue.lock() {
                                        queue.push(msg);
                                    }
                                }
                            }
                        } else {
                            eprintln!("[主机] 反序列化消息失败，数据大小: {}", size);
                            // 打印原始数据的前几个字节用于调试
                            let preview = &buf[..size.min(20)];
                            // 调试输出已禁用: println!("[主机] 原始数据预览: {:?}", preview);
                        }
                    }
                    Err(e) => {
                        // 非阻塞模式下，没有数据是正常的
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                        // 如果socket已关闭，退出线程
                        if e.kind() == std::io::ErrorKind::NotConnected || 
                           e.kind() == std::io::ErrorKind::BrokenPipe {
                            // 调试输出已禁用: println!("[主机] Socket连接已断开，退出接收线程");
                            break;
                        }
                            eprintln!("[主机] 接收数据错误: {}", e);
                    }
                }
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
    });
}

/// 搜索房间（作为客户端）- WSL2优化版本，支持手动输入IP
pub fn search_room(
    mut network_manager: ResMut<NetworkManager>,
    mut room_info: ResMut<RoomInfo>,
) {
    network_manager.is_host = false;
    room_info.is_host = false;
    
    // 检查是否有手动输入的IP地址
    let manual_ip = network_manager.manual_ip.lock().unwrap().clone();
    if let Some(ip_address) = manual_ip {
        // 调试输出已禁用: println!("[客户端] 使用手动输入的IP地址: {}", ip_address);
        
        // 解析IP地址和端口
        let target_addr = if ip_address.contains(':') {
            match ip_address.parse::<SocketAddr>() {
                Ok(addr) => addr,
                Err(e) => {
                    eprintln!("[客户端] 解析IP地址失败: {} - {}", ip_address, e);
                    return;
                }
            }
        } else {
            // 如果没有端口，使用默认端口12345
            match format!("{}:12345", ip_address).parse::<SocketAddr>() {
                Ok(addr) => addr,
                Err(e) => {
                    eprintln!("[客户端] 解析IP地址失败: {} - {}", ip_address, e);
                    return;
                }
            }
        };
        
        // 调试输出已禁用: println!("[客户端] 目标地址: {}", target_addr);
        
        // 如果已经有socket，先清理它
        if network_manager.socket.is_some() {
            // 调试输出已禁用: println!("[客户端] 检测到已有socket，先清理...");
            network_manager.is_running.store(false, Ordering::Relaxed);
            network_manager.socket = None;
            // 等待一小段时间确保旧socket完全关闭
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        
        // 创建UDP socket
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => {
                // 调试输出已禁用: println!("[客户端] UDP socket绑定成功（自动分配端口）");
                s
            }
            Err(e) => {
                eprintln!("[客户端] UDP socket绑定失败: {}", e);
                return;
            }
        };
        socket.set_broadcast(true).expect("无法启用广播");
        socket.set_nonblocking(true).expect("无法设置非阻塞模式");
        // 调试输出已禁用: println!("[客户端] UDP socket配置完成: broadcast=true, nonblocking=true");
        
        let socket_arc = Arc::new(Mutex::new(socket));
        network_manager.is_running.store(true, Ordering::Relaxed);
        network_manager.socket = Some(socket_arc.clone());
        
        // 立即保存远程地址
        *network_manager.remote_addr.lock().unwrap() = Some(target_addr);
        
        let message_queue = network_manager.message_queue.clone();
        let room_id_arc = network_manager.room_id.clone();
        let remote_addr_arc = network_manager.remote_addr.clone();
        let room_found = Arc::new(Mutex::new(false));
        let is_running_flag = network_manager.is_running.clone();
        
        // 定期发送房间发现请求到指定IP
        let socket_for_send = socket_arc.clone();
        let target_addr_clone = target_addr;
        let send_running_flag = is_running_flag.clone();
        thread::spawn(move || {
            let discovery_msg = NetworkMessage::RoomDiscoveryRequest;
            let mut request_count = 0u32;
            loop {
                if !send_running_flag.load(Ordering::Relaxed) {
                    // 调试输出已禁用: println!("[客户端] 发送线程收到停止信号，退出");
                    break;
                }
                request_count += 1;
                if let Ok(data) = bincode::serialize(&discovery_msg) {
                    if let Ok(socket_guard) = socket_for_send.lock() {
                        // 直接发送到指定IP地址
                        match socket_guard.send_to(&data, target_addr_clone) {
                            Ok(_sent) => {
                                if request_count % 10 == 0 {
                                    // 调试输出已禁用: println!("[客户端] 已发送房间发现请求到 {} (第{}次)", target_addr_clone, request_count);
                                }
                            }
                            Err(e) => {
                                if request_count % 10 == 0 {
                                    eprintln!("[客户端] 发送房间发现请求失败: {}", e);
                                }
                            }
                        }
                    }
                }
                thread::sleep(std::time::Duration::from_millis(500)); // 每0.5秒发送一次
            }
        });
        
        // 接收线程（与下面的代码相同）
        let recv_running_flag = is_running_flag.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                if !recv_running_flag.load(Ordering::Relaxed) {
                    // 调试输出已禁用: println!("[客户端] 接收线程收到停止信号，退出");
                    break;
                }
                if let Ok(socket_guard) = socket_arc.lock() {
                    let recv_result = socket_guard.recv_from(&mut buf);
                    drop(socket_guard);
                    match recv_result {
                        Ok((size, addr)) => {
                            // println!("[客户端] 收到来自 {} 的数据，大小: {} 字节", addr, size); // 已禁用：日志太多
                            // 保存远程地址
                            *remote_addr_arc.lock().unwrap() = Some(addr);
                            
                            if let Ok(msg) = bincode::deserialize::<NetworkMessage>(&buf[..size]) {
                                // println!("[客户端] 收到消息: {:?}", msg); // 已禁用：日志太多
                                match msg {
                                    NetworkMessage::RoomDiscoveryResponse { room_id, .. } => {
                                        let mut found = room_found.lock().unwrap();
                                        if !*found {
                                            *found = true;
                                            // 调试输出已禁用: println!("[客户端] 发现房间: {}", room_id);
                                            *room_id_arc.lock().unwrap() = room_id.clone();
                                            
                                            // 使用接收到的地址（addr）直接发送加入请求
                                            let join_msg = NetworkMessage::JoinRequest {
                                                room_id: room_id.clone(),
                                            };
                                            if let Ok(data) = bincode::serialize(&join_msg) {
                                                if let Ok(socket_guard) = socket_arc.lock() {
                                                    match socket_guard.send_to(&data, addr) {
                                                        Ok(_) => {}, // 调试输出已禁用: println!("[客户端] 已发送加入请求到 {}，发送了 {} 字节", addr, sent),
                                                        Err(e) => eprintln!("[客户端] 发送加入请求失败: {}", e),
                                                    }
                                                }
                                            } else {
                                                eprintln!("[客户端] 序列化加入请求失败");
                                            }
                                            
                                            // 保存房间信息到队列
                                            if let Ok(mut queue) = message_queue.lock() {
                                                queue.push(NetworkMessage::RoomDiscoveryResponse {
                                                    room_id,
                                                    player_name: "Client".to_string(),
                                                });
                                            }
                                        }
                                    }
                                    NetworkMessage::JoinAccept { player_id } => {
                                        // 调试输出已禁用: println!("[客户端] 成功加入房间！玩家ID: {:?}", player_id);
                                        if let Ok(mut queue) = message_queue.lock() {
                                            queue.push(NetworkMessage::JoinAccept { player_id });
                                        }
                                    }
                                    _ => {
                                        // 将消息放入队列（远程地址已在收到消息时保存）
                                        // println!("[客户端] 收到消息: {:?}", msg); // 已禁用：日志太多
                                        if let Ok(mut queue) = message_queue.lock() {
                                            queue.push(msg);
                                        }
                                    }
                                }
                            } else {
                                eprintln!("[客户端] 反序列化消息失败，数据大小: {}", size);
                            }
                        }
                        Err(e) => {
                            // 非阻塞模式下，没有数据是正常的
                            if e.kind() != std::io::ErrorKind::WouldBlock {
                                eprintln!("[客户端] 接收数据错误: {}", e);
                            }
                        }
                    }
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        
        return; // 手动IP模式，直接返回
    }
    
    // 自动搜索模式（原有逻辑）
    // 获取本地IP地址
    let local_ips = get_local_ip_addresses();
    if let Some(&local_ip) = local_ips.first() {
        *network_manager.local_ip.lock().unwrap() = Some(local_ip);
        // 调试输出已禁用: println!("[客户端] 本地IP地址: {}", local_ip);
        
        // 计算子网范围
        let subnet_ips = get_subnet_ips(local_ip, 20); // WSL2通常使用/20子网
        // 调试输出已禁用: println!("[客户端] 将扫描 {} 个可能的IP地址", subnet_ips.len());
        
        // 创建UDP socket
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => {
                // 调试输出已禁用: println!("[客户端] UDP socket绑定成功（自动分配端口）");
                s
            }
            Err(e) => {
                eprintln!("[客户端] UDP socket绑定失败: {}", e);
                panic!("无法绑定UDP端口: {}", e);
            }
        };
        socket.set_broadcast(true).expect("无法启用广播");
        socket.set_nonblocking(true).expect("无法设置非阻塞模式");
        // 调试输出已禁用: println!("[客户端] UDP socket配置完成: broadcast=true, nonblocking=true");
        
        // 打印本地地址信息（用于调试）
        if let Ok(local_addr) = socket.local_addr() {
            // 调试输出已禁用: println!("[客户端] 绑定地址: {}", local_addr);
        }
        
        let socket_arc = Arc::new(Mutex::new(socket));
        network_manager.is_running.store(true, Ordering::Relaxed);
        network_manager.socket = Some(socket_arc.clone());
        network_manager.is_running.store(true, Ordering::Relaxed);
        
        // 调试输出已禁用: println!("正在搜索房间...");
        // 调试输出已禁用: println!("[客户端] WSL2环境：将尝试广播和直接扫描");
        
        let message_queue = network_manager.message_queue.clone();
        let room_id_arc = network_manager.room_id.clone();
        let remote_addr_arc = network_manager.remote_addr.clone();
        let room_found = Arc::new(Mutex::new(false));
        let subnet_ips_arc = Arc::new(subnet_ips);
        let is_running_flag = network_manager.is_running.clone();
        
        // 定期发送房间发现请求
        let socket_for_send = socket_arc.clone();
        let subnet_ips_clone = subnet_ips_arc.clone();
        let send_running_flag = is_running_flag.clone();
        thread::spawn(move || {
            let discovery_msg = NetworkMessage::RoomDiscoveryRequest;
            let mut request_count = 0u32;
            loop {
                if !send_running_flag.load(Ordering::Relaxed) {
                    // 调试输出已禁用: println!("[客户端] 广播发送线程收到停止信号，退出");
                    break;
                }
                request_count += 1;
                if let Ok(data) = bincode::serialize(&discovery_msg) {
                    if let Ok(socket_guard) = socket_for_send.lock() {
                        // 1. 尝试标准广播地址
                        let broadcast_addresses = vec![
                            SocketAddr::new(Ipv4Addr::BROADCAST.into(), 12345),
                            SocketAddr::new("255.255.255.255".parse().unwrap(), 12345),
                        ];
                        
                        for broadcast_addr in &broadcast_addresses {
                            let _ = socket_guard.send_to(&data, *broadcast_addr);
                        }
                        
                        // 2. 每5次请求后，扫描子网中的IP地址（WSL2需要）
                        if request_count % 5 == 0 {
                            let subnet_ips = subnet_ips_clone.clone();
                            // 分批发送，避免一次性发送太多
                            let batch_size = 50;
                            let start_idx = ((request_count / 5) as usize * batch_size) % subnet_ips.len();
                            let end_idx = (start_idx + batch_size).min(subnet_ips.len());
                            
                            for i in start_idx..end_idx {
                                let target_ip = subnet_ips[i];
                                let target_addr = SocketAddr::new(target_ip.into(), 12345);
                                let _ = socket_guard.send_to(&data, target_addr);
                            }
                            
                            if request_count % 20 == 0 {
                                // 调试输出已禁用: println!("[客户端] 已扫描 {}/{} 个IP地址 (第{}次请求)", end_idx, subnet_ips.len(), request_count);
                            }
                        }
                    }
                }
                thread::sleep(std::time::Duration::from_millis(500)); // 每0.5秒发送一次
            }
        });
        
        // 接收线程
        let recv_running_flag = is_running_flag.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                if !recv_running_flag.load(Ordering::Relaxed) {
                    // 调试输出已禁用: println!("[客户端] 广播接收线程收到停止信号，退出");
                    break;
                }
                if let Ok(socket_guard) = socket_arc.lock() {
                    let recv_result = socket_guard.recv_from(&mut buf);
                    drop(socket_guard);
                    match recv_result {
                        Ok((size, addr)) => {
                            // println!("[客户端] 收到来自 {} 的数据，大小: {} 字节", addr, size); // 已禁用：日志太多
                            // 保存远程地址
                            *remote_addr_arc.lock().unwrap() = Some(addr);
                            
                            if let Ok(msg) = bincode::deserialize::<NetworkMessage>(&buf[..size]) {
                                // println!("[客户端] 收到消息: {:?}", msg); // 已禁用：日志太多
                                match msg {
                                    NetworkMessage::RoomDiscoveryResponse { room_id, .. } => {
                                        let mut found = room_found.lock().unwrap();
                                        if !*found {
                                            *found = true;
                                            // 调试输出已禁用: println!("[客户端] 发现房间: {}", room_id);
                                            *room_id_arc.lock().unwrap() = room_id.clone();
                                            
                                            // 使用接收到的地址（addr）直接发送加入请求
                                            let join_msg = NetworkMessage::JoinRequest {
                                                room_id: room_id.clone(),
                                            };
                                            if let Ok(data) = bincode::serialize(&join_msg) {
                                                if let Ok(socket_guard) = socket_arc.lock() {
                                                    match socket_guard.send_to(&data, addr) {
                                                        Ok(_) => {}, // 调试输出已禁用: println!("[客户端] 已发送加入请求到 {}，发送了 {} 字节", addr, sent),
                                                        Err(e) => eprintln!("[客户端] 发送加入请求失败: {}", e),
                                                    }
                                                }
                                            } else {
                                                eprintln!("[客户端] 序列化加入请求失败");
                                            }
                                            
                                            // 保存房间信息到队列
                                            if let Ok(mut queue) = message_queue.lock() {
                                                queue.push(NetworkMessage::RoomDiscoveryResponse {
                                                    room_id,
                                                    player_name: "Client".to_string(),
                                                });
                                            }
                                        }
                                    }
                                    NetworkMessage::JoinAccept { player_id } => {
                                        // 调试输出已禁用: println!("[客户端] 成功加入房间！玩家ID: {:?}", player_id);
                                        if let Ok(mut queue) = message_queue.lock() {
                                            queue.push(NetworkMessage::JoinAccept { player_id });
                                        }
                                    }
                                    _ => {
                                        // 将消息放入队列（远程地址已在收到消息时保存）
                                        // println!("[客户端] 收到消息: {:?}", msg); // 已禁用：日志太多
                                        if let Ok(mut queue) = message_queue.lock() {
                                            queue.push(msg);
                                        }
                                    }
                                }
                            } else {
                                eprintln!("[客户端] 反序列化消息失败，数据大小: {}", size);
                                // 打印原始数据的前几个字节用于调试
                                let preview = &buf[..size.min(20)];
                                // 调试输出已禁用: println!("[客户端] 原始数据预览: {:?}", preview);
                            }
                        }
                        Err(e) => {
                            // 非阻塞模式下，没有数据是正常的
                            if e.kind() != std::io::ErrorKind::WouldBlock {
                                eprintln!("[客户端] 接收数据错误: {}", e);
                            }
                        }
                    }
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
    } else {
        eprintln!("[客户端] 无法获取本地IP地址，仅使用广播方式");
        // 回退到原来的广播方式
        let socket = UdpSocket::bind("0.0.0.0:0").expect("无法绑定UDP端口");
        socket.set_broadcast(true).expect("无法启用广播");
        socket.set_nonblocking(true).expect("无法设置非阻塞模式");
        let socket_arc = Arc::new(Mutex::new(socket));
        network_manager.socket = Some(socket_arc.clone());
        network_manager.is_running.store(true, Ordering::Relaxed);
        
        let message_queue = network_manager.message_queue.clone();
        let room_id_arc = network_manager.room_id.clone();
        let remote_addr_arc = network_manager.remote_addr.clone();
        let room_found = Arc::new(Mutex::new(false));
        let is_running_flag = network_manager.is_running.clone();
        
        let socket_for_send = socket_arc.clone();
        let send_running_flag = is_running_flag.clone();
        thread::spawn(move || {
            let discovery_msg = NetworkMessage::RoomDiscoveryRequest;
            loop {
                if !send_running_flag.load(Ordering::Relaxed) {
                    // 调试输出已禁用: println!("[客户端] 回退模式发送线程收到停止信号，退出");
                    break;
                }
                if let Ok(data) = bincode::serialize(&discovery_msg) {
                    let broadcast_addr = SocketAddr::new(Ipv4Addr::BROADCAST.into(), 12345);
                    if let Ok(socket_guard) = socket_for_send.lock() {
                        let _ = socket_guard.send_to(&data, broadcast_addr);
                    }
                }
                thread::sleep(std::time::Duration::from_millis(1000));
            }
        });
        
        let recv_running_flag = is_running_flag.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                if !recv_running_flag.load(Ordering::Relaxed) {
                    // 调试输出已禁用: println!("[客户端] 回退模式接收线程收到停止信号，退出");
                    break;
                }
                if let Ok(socket_guard) = socket_arc.lock() {
                    let recv_result = socket_guard.recv_from(&mut buf);
                        drop(socket_guard);
                    if let Ok((size, addr)) = recv_result {
                        *remote_addr_arc.lock().unwrap() = Some(addr);
                        if let Ok(msg) = bincode::deserialize::<NetworkMessage>(&buf[..size]) {
                            match msg {
                                NetworkMessage::RoomDiscoveryResponse { room_id, .. } => {
                                    let mut found = room_found.lock().unwrap();
                                    if !*found {
                                        *found = true;
                                        *room_id_arc.lock().unwrap() = room_id.clone();
                                        if let Ok(mut queue) = message_queue.lock() {
                                            queue.push(NetworkMessage::RoomDiscoveryResponse {
                                                room_id,
                                                player_name: "Client".to_string(),
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
    }
}

/// 延迟执行search_room（等待用户输入IP或自动搜索）
pub fn search_room_delayed(
    mut network_manager: ResMut<NetworkManager>,
    mut room_info: ResMut<RoomInfo>,
    mut has_searched: Local<bool>,
) {
    // 只在第一次执行
    if !*has_searched {
        *has_searched = true;
        // 等待一小段时间，让用户有机会输入IP
        std::thread::sleep(std::time::Duration::from_millis(100));
        search_room(network_manager, room_info);
    }
}

/// 处理网络消息
pub fn handle_network_messages(
    network_manager: Res<NetworkManager>,
    mut room_info: ResMut<RoomInfo>,
    mut app_state: ResMut<NextState<AppState>>,
) {
    // 处理接收到的消息
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        let mut deferred = Vec::new();
        while let Some(msg) = queue.pop() {
            match msg {
                NetworkMessage::RoomDiscoveryResponse { room_id, .. } => {
                    *network_manager.room_id.lock().unwrap() = room_id.clone();
                    room_info.room_code = Some(room_id.clone());
                    // 调试输出已禁用: println!("收到房间发现响应，房间ID: {}", room_id);
                }
                NetworkMessage::JoinAccept { player_id: _ } => {
                    room_info.is_connected = true;
                    // 调试输出已禁用: println!("房间连接已建立");
                    // 客户端自动进入房间等待状态
                    if !network_manager.is_host {
                        // 调试输出已禁用: println!("[客户端] 已加入房间，等待房主开始游戏...");
                        app_state.set(AppState::InRoom);
                    } else {
                        // 房主收到JoinAccept消息（自己的），更新UI状态
                        // 调试输出已禁用: println!("[房主] 客户端已加入房间");
                    }
                }
                NetworkMessage::StartGame => {
                    // 调试输出已禁用: println!("[客户端] 收到开始游戏消息，切换到Playing状态");
                    if !network_manager.is_host {
                        room_info.is_connected = true;
                        let remote_addr = network_manager.remote_addr.lock().unwrap();
                        // 调试输出已禁用: println!("[客户端] remote_addr: {:?}, socket: {:?}", *remote_addr, if network_manager.socket.is_some() { "已设置" } else { "未设置" });
                        app_state.set(AppState::Playing);
                        // 调试输出已禁用: println!("[客户端] 状态已切换到Playing");
                    } else {
                        // 调试输出已禁用: println!("[房主] 收到StartGame消息（可能是重复消息）");
                    }
                }
                NetworkMessage::SwitchRoles { .. } => {
                    // SwitchRoles消息由handle_game_state_system处理
                    // 将消息放回队列，让handle_game_state_system处理
                    deferred.push(msg);
                }
                NetworkMessage::DefenderState { .. } | 
                NetworkMessage::CrosshairPosition { .. } |
                NetworkMessage::GameState { .. } |
                NetworkMessage::RoundInfoSync { .. } |
                NetworkMessage::BulletSpawn { .. } |
                NetworkMessage::HealthUpdate { .. } |
                NetworkMessage::GameOver { .. } => {
                    // 这些消息由专门的系统处理，放回队列
                    deferred.push(msg);
                }
                _ => {
                    // 其他消息处理
                }
            }
        }
        while let Some(msg) = deferred.pop() {
            queue.push(msg);
        }
    }
}

/// 发送网络消息
pub fn send_network_message(
    network_manager: &NetworkManager,
    message: NetworkMessage,
) {
    if let Some(socket_arc) = &network_manager.socket {
        if let Ok(remote_addr_guard) = network_manager.remote_addr.lock() {
            if let Some(remote_addr) = *remote_addr_guard {
                if let Ok(data) = bincode::serialize(&message) {
                    if let Ok(socket_guard) = socket_arc.lock() {
                        match socket_guard.send_to(&data, remote_addr) {
                            Ok(_sent) => {
                                // 调试：每60帧打印一次（约1秒）
                                // println!("[网络] 发送消息成功: {:?} -> {} ({} 字节)", std::any::type_name_of_val(&message), remote_addr, _sent);
                            }
                            Err(e) => {
                                eprintln!("[网络] 发送消息失败: {:?} -> {}: {}", 
                                         message, remote_addr, e);
                        }
                    }
                    } else {
                        eprintln!("[网络] 无法锁定socket，无法发送消息: {:?}", message);
                    }
                } else {
                    eprintln!("[网络] 序列化消息失败: {:?}", message);
                }
            } else {
                // remote_addr 为 None，说明连接尚未建立
                // 对于某些消息（如GameState），这是正常的，因为可能还在等待连接
                // 但对于游戏中的消息（如DefenderState、CrosshairPosition），这可能是问题
                eprintln!("[网络] 警告：remote_addr 为 None，无法发送消息: {:?}", message);
            }
        } else {
            eprintln!("[网络] 无法锁定remote_addr，无法发送消息: {:?}", message);
        }
    } else {
        eprintln!("[网络] 警告：socket 为 None，无法发送消息: {:?}", message);
    }
}

/// 清理网络资源
pub fn cleanup_network(
    mut network_manager: ResMut<NetworkManager>,
) {
    network_manager.is_running.store(false, Ordering::Relaxed);
    network_manager.socket = None;
    *network_manager.remote_addr.lock().unwrap() = None;
    *network_manager.room_id.lock().unwrap() = String::new();
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        queue.clear();
    }
}

// ========== 游戏状态同步系统 ==========

use crate::gameplay::{Health, RoundInfo, CursorPosition, CrosshairOffset, DodgeAction};
use crate::ViewConfig;

/// 主机：发送游戏状态（位置、血量、角色等）
pub fn sync_game_state_system(
    network_manager: Res<NetworkManager>,
    player_query: Query<(&crate::PlayerId, &Transform, &Health, &crate::PlayerRole), (With<crate::PlayerId>, With<crate::PlayerRole>)>,
    round_info: Res<RoundInfo>,
    time: Res<Time>,
    mut sync_timer: Local<f32>,
) {
    // 只有主机才发送游戏状态
    if !network_manager.is_host {
        return;
    }
    
    // 每 0.05 秒（20次/秒）同步一次
    *sync_timer += time.delta_seconds();
    if *sync_timer < 0.05 {
        return;
    }
    *sync_timer = 0.0;
    
    // 收集所有玩家的状态
    let mut player_positions = Vec::new();
    let mut player_roles = Vec::new();
    let mut health = Vec::new();
    
    let player_count = player_query.iter().count();
    if player_count == 0 {
        // 如果还没有玩家实体，不发送状态
        return;
    }
    
    for (player_id, transform, health_comp, role) in player_query.iter() {
        let pos = transform.translation;
        player_positions.push((*player_id, [pos.x, pos.y, pos.z]));
        player_roles.push((*player_id, *role));
        health.push((*player_id, health_comp.0));
    }
    
    // 发送游戏状态
    let game_state = NetworkMessage::GameState {
        player_positions,
        player_roles,
        health,
    };
    send_network_message(&*network_manager, game_state);
    
    // 发送回合信息
    let round_info_sync = NetworkMessage::RoundInfoSync {
        current_attacker: round_info.current_attacker,
        bullets_left: round_info.bullets_left as u32,
        round_timer_remaining: round_info.round_timer.remaining_secs(),
        p1_health: round_info.p1_health,
        p2_health: round_info.p2_health,
        bullets_fired: round_info.bullets_fired_this_round as u32,
        bullets_hit: round_info.bullets_hit_defender as u32,
    };
    send_network_message(&*network_manager, round_info_sync);
}

/// 强制同步游戏状态（用于游戏结束前确保数据同步）
/// 接受玩家数据列表和回合信息，直接发送同步消息
pub fn force_sync_game_state(
    network_manager: &NetworkManager,
    player_data: &[(crate::PlayerId, [f32; 3], f32, crate::PlayerRole)], // (player_id, position, health, role)
    round_info: &RoundInfo,
) {
    // 只有主机才发送游戏状态
    if !network_manager.is_host {
        return;
    }
    
    if player_data.is_empty() {
        // 如果还没有玩家数据，不发送状态
        return;
    }
    
    // 收集所有玩家的状态
    let mut player_positions = Vec::new();
    let mut player_roles = Vec::new();
    let mut health = Vec::new();
    
    for (player_id, pos, health_val, role) in player_data.iter() {
        player_positions.push((*player_id, *pos));
        player_roles.push((*player_id, *role));
        health.push((*player_id, *health_val));
    }
    
    // 发送游戏状态
    let game_state = NetworkMessage::GameState {
        player_positions,
        player_roles,
        health,
    };
    send_network_message(network_manager, game_state);
    
    // 发送回合信息（包含最新的血量）
    let round_info_sync = NetworkMessage::RoundInfoSync {
        current_attacker: round_info.current_attacker,
        bullets_left: round_info.bullets_left as u32,
        round_timer_remaining: round_info.round_timer.remaining_secs(),
        p1_health: round_info.p1_health,
        p2_health: round_info.p2_health,
        bullets_fired: round_info.bullets_fired_this_round as u32,
        bullets_hit: round_info.bullets_hit_defender as u32,
    };
    send_network_message(network_manager, round_info_sync);
    
    // 调试输出已禁用: println!("[强制同步] 已发送最终游戏状态同步，P1血量: {:.1}, P2血量: {:.1}", round_info.p1_health, round_info.p2_health);
}

/// 客户端：接收并应用游戏状态
pub fn handle_game_state_system(
    mut commands: Commands,
    network_manager: Res<NetworkManager>,
    mut player_query: Query<(Entity, &crate::PlayerId, &mut Transform, &mut Health, &mut crate::PlayerRole), (With<crate::PlayerId>, With<crate::PlayerRole>)>,
    mut round_info: ResMut<RoundInfo>,
    mut view_config: ResMut<ViewConfig>,
    mut camera_switch_writer: EventWriter<crate::gameplay::CameraSwitchEvent>,
    all_cameras_query: Query<Entity, With<Camera2d>>, // 用于移除组件
) {
    // 只有客户端才处理游戏状态
    if network_manager.is_host {
        return;
    }
    
    // 处理接收到的消息
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        let messages: Vec<NetworkMessage> = queue.drain(..).collect();
        let mut messages_to_keep = Vec::new();
        for msg in messages {
            match msg {
                NetworkMessage::GameState {
                    player_positions,
                    player_roles,
                    health,
                } => {
                    // 更新玩家位置
                    // 注意：客户端（防守方）不应该从GameState更新自己的防守方位置
                    // 因为防守方位置应该由本地移动系统控制
                    let local_player_id = if network_manager.is_host {
                        crate::PlayerId::Player1
                    } else {
                        crate::PlayerId::Player2
                    };
                    
                    for (player_id, [x, y, z]) in player_positions {
                        // 更新所有玩家的位置（包括本地玩家和对方玩家）
                        // 注意：防守方位置主要由DefenderState实时同步，但GameState用于初始同步和位置校正
                        // 进攻方位置固定，但也需要同步以确保一致性
                        for (_entity, pid, mut transform, _, role) in player_query.iter_mut() {
                            if *pid == player_id {
                                // 如果是防守方，且是本地玩家，则跳过（由本地移动系统控制）
                                // 但如果是对方玩家，则更新（DefenderState会实时更新，但GameState用于初始同步）
                                if *pid == local_player_id && matches!(*role, crate::PlayerRole::Defender) {
                                    // 本地防守方位置由本地移动系统控制，不从这里更新
                                    continue;
                                }
                                
                                // 更新位置（包括本地进攻方和对方玩家）
                                transform.translation = Vec3::new(x, y, z);
                                break;
                            }
                        }
                    }
                    
                    // 更新玩家角色（包括位置和视图）
                    for (player_id, role) in player_roles {
                        for (entity, pid, mut transform, _, mut player_role) in player_query.iter_mut() {
                            if *pid == player_id {
                                let old_role = *player_role;
                                *player_role = role;
                                if old_role != role {
                                    // 调试输出已禁用: println!("[客户端] 玩家 {:?} 角色已更新: {:?} -> {:?}", player_id, old_role, role);
                                    // 角色切换时，更新位置
                                    match role {
                                        crate::PlayerRole::Attacker => {
                                            transform.translation = crate::ATTACKER_START_POS;
                                        }
                                        crate::PlayerRole::Defender => {
                                            transform.translation = crate::DEFENDER_START_POS;
                                        }
                                    }
                                    // 更新视图配置：根据本地玩家（Player2）的角色来更新视图
                                    // 视图配置完全绑定在角色上，而不是绑定在玩家身份上
                                    if player_id == crate::PlayerId::Player2 {
                                        let new_is_attacker = matches!(role, crate::PlayerRole::Attacker);
                                        view_config.is_attacker_view = new_is_attacker;
                                        // 调试输出已禁用: println!("[客户端] 本地玩家角色已更新，视图切换到{}视图 (基于角色，而不是玩家身份)", if new_is_attacker { "进攻方" } else { "防守方" });
                                    }
                                }
                                break;
                            }
                        }
                    }
                    
                    // 更新血量（包括本地玩家和对方玩家）
                    for (player_id, health_value) in health {
                        for (entity, pid, _, mut health_comp, _) in player_query.iter_mut() {
                            if *pid == player_id {
                                health_comp.0 = health_value;
                                // 同时更新回合信息中的血量
                                match player_id {
                                    crate::PlayerId::Player1 => {
                                        round_info.p1_health = health_value;
                                    }
                                    crate::PlayerId::Player2 => {
                                        round_info.p2_health = health_value;
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
                NetworkMessage::RoundInfoSync {
                    current_attacker,
                    bullets_left,
                    round_timer_remaining: _,
                    p1_health,
                    p2_health,
                    bullets_fired,
                    bullets_hit,
                } => {
                    round_info.current_attacker = current_attacker;
                    round_info.bullets_left = bullets_left as i32;
                    round_info.p1_health = p1_health;
                    round_info.p2_health = p2_health;
                    round_info.bullets_fired_this_round = bullets_fired as i32;
                    round_info.bullets_hit_defender = bullets_hit as i32;
                }
                NetworkMessage::SwitchRoles { new_attacker } => {
                    // 调试输出已禁用: println!("[客户端] 收到角色切换消息，新的进攻方: {:?}", new_attacker);
                    // 只有在当前不是切换状态时，才处理角色切换消息
                    // 避免重复处理导致的状态循环
                    if round_info.current_attacker != new_attacker {
                        // 更新回合信息，标记需要角色切换
                        round_info.current_attacker = new_attacker;
                        round_info.is_switching = true;
                        
                        // 立即根据本地玩家角色更新视图配置与相机
                        let local_player = crate::PlayerId::Player2;
                        let new_is_attacker = new_attacker == local_player;
                        let old_is_attacker = view_config.is_attacker_view;
                        
                        // 调试输出已禁用: println!("[客户端] ========== 收到角色切换消息，立即更新视图 ==========");
                        // 调试输出已禁用: println!("[客户端] 新的进攻方: {:?}", new_attacker);
                        // 调试输出已禁用: println!("[客户端] 本地玩家: {:?}", local_player);
                        // 调试输出已禁用: println!("[客户端] 旧视图配置: is_attacker_view = {} ({})", old_is_attacker, if old_is_attacker { "进攻方视图" } else { "防守方视图" });
                        // 调试输出已禁用: println!("[客户端] 新视图配置: is_attacker_view = {} ({})", new_is_attacker, if new_is_attacker { "进攻方视图" } else { "防守方视图" });
                        
                        view_config.is_attacker_view = new_is_attacker;
                        
                        camera_switch_writer.send(crate::gameplay::CameraSwitchEvent {
                            is_attacker_view: new_is_attacker,
                        });
                        // 调试输出已禁用: println!("[客户端] 已发送 CameraSwitchEvent");
                        
                        // 调试输出已禁用: println!("[客户端] ========== 开始强制切换相机 ==========");
                        // 使用直接切换方法，避免查询延迟
                        crate::gameplay::apply_network_camera_view_direct(
                            &mut commands,
                            &all_cameras_query,
                            &mut view_config,
                            new_is_attacker,
                        );
                        // 调试输出已禁用: println!("[客户端] ========== 相机切换完成 ==========");
                    } else {
                        // 调试输出已禁用: println!("[客户端] 忽略重复的角色切换消息，current_attacker 已经是 {:?}", new_attacker);
                    }
                    
                    // 注意：不将消息放回队列，因为我们已经处理了
                    // handle_client_role_switch 会检查 is_switching 标志
                }
                NetworkMessage::GameOver { winner } => {
                    // 客户端收到游戏结束消息，触发游戏结束状态
                    // 调试输出已禁用: println!("[游戏结束调试] handle_game_state_system 收到GameOver消息: winner={:?}", winner);
                    // 这个消息会在 handle_game_over_network_system 中处理
                    // 放回队列，让 handle_game_over_network_system 处理（它会在下一帧运行）
                    messages_to_keep.push(msg);
                    // 调试输出已禁用: println!("[游戏结束调试] GameOver消息已放回队列，等待 handle_game_over_network_system 处理");
                }
                NetworkMessage::BulletSpawn { .. } |
                NetworkMessage::HealthUpdate { .. } => {
                    // 这些消息由专门的系统处理，放回队列
                    messages_to_keep.push(msg);
                }
                _ => {
                    // 其他消息放回队列，由 handle_network_messages 处理
                    messages_to_keep.push(msg);
                }
            }
        }
        // 将需要保留的消息放回队列
        queue.extend(messages_to_keep);
    }
}

/// 客户端：处理角色切换（触发RoundState::Switching）
pub fn handle_client_role_switch(
    network_manager: Res<NetworkManager>,
    mut next_round_state: ResMut<NextState<crate::RoundState>>,
    round_info: Res<RoundInfo>,
    mut view_config: ResMut<ViewConfig>,
    bullet_query: Query<Entity, With<crate::gameplay::Bullet>>,
    round_state: Res<State<crate::RoundState>>,
    mut has_triggered_switch: Local<bool>,
) {
    // 只有客户端才处理
    if network_manager.is_host {
        return;
    }
    
    // 如果已经在切换状态，重置标志并返回
    if *round_state.get() == crate::RoundState::Switching {
        *has_triggered_switch = false; // 重置标志，等待切换完成
        return;
    }
    
    // 如果当前状态不是 Attacking，重置标志并返回
    if *round_state.get() != crate::RoundState::Attacking {
        *has_triggered_switch = false;
        return;
    }
    
    // 如果已经触发过切换，不再重复触发
    if *has_triggered_switch {
        return;
    }
    
    // 如果标记为需要切换，且所有子弹都消失了，触发角色切换
    // 注意：这里不需要检查 is_switching，因为客户端收到 SwitchRoles 消息后，
    // handle_game_state_system 已经设置了 is_switching = true
    // 但是我们需要等待所有子弹消失
    // 注意：视图配置会在 switch_roles_system 中根据实际角色更新，而不是在这里更新
    if round_info.is_switching && bullet_query.is_empty() {
        // 标记已经触发切换，防止重复触发
        *has_triggered_switch = true;
        
        next_round_state.set(crate::RoundState::Switching);
        // 调试输出已禁用: println!("[客户端] 触发RoundState::Switching，新的进攻方: {:?} (视图配置将在角色切换后更新)", round_info.current_attacker);
    } else {
        // 如果条件不满足，重置标志
        *has_triggered_switch = false;
    }
}

/// 防守方：发送防守方状态
/// 基于视图配置（角色），而不是基于玩家身份（房主/客户端）
pub fn sync_player_input_system(
    network_manager: Res<NetworkManager>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    _cursor_pos: Res<CursorPosition>,
    _crosshair_offset: Res<CrosshairOffset>,
    player_query: Query<(&crate::PlayerId, &Transform, &DodgeAction, &crate::PlayerRole), (With<crate::PlayerId>, With<crate::PlayerRole>)>,
    mut input_timer: Local<f32>,
    time: Res<Time>,
    view_config: Res<ViewConfig>,
    room_info: Res<crate::RoomInfo>,
    app_state: Res<State<crate::AppState>>,
) {
    // 只有网络模式才发送
    if !room_info.is_connected {
        return;
    }
    
    // 如果游戏已结束，不允许发送输入
    if *app_state.get() == crate::AppState::GameOver {
        return;
    }
    
    // 基于视图配置（角色），而不是基于玩家身份
    // 只有当前视图是防守方视图时，才发送防守方状态
    if view_config.is_attacker_view {
        return;
    }
    
    // 每帧发送输入（或可以降低频率）
    *input_timer += time.delta_seconds();
    if *input_timer < 0.016 {  // 约60fps
        return;
    }
    *input_timer = 0.0;
    
    // 确定本地玩家ID（用于发送消息）
    let local_player_id = if network_manager.is_host {
        crate::PlayerId::Player1
    } else {
        crate::PlayerId::Player2
    };
    
    // 收集防守方的移动输入
    let mut movement = None;
    // 使用WASD移动
    if keyboard_input.pressed(KeyCode::KeyW) || keyboard_input.pressed(KeyCode::KeyS) ||
       keyboard_input.pressed(KeyCode::KeyA) || keyboard_input.pressed(KeyCode::KeyD) {
        let mut move_dir = Vec2::ZERO;
        if keyboard_input.pressed(KeyCode::KeyW) { move_dir.y += 1.0; }
        if keyboard_input.pressed(KeyCode::KeyS) { move_dir.y -= 1.0; }
        if keyboard_input.pressed(KeyCode::KeyA) { move_dir.x -= 1.0; }
        if keyboard_input.pressed(KeyCode::KeyD) { move_dir.x += 1.0; }
        if move_dir.length_squared() > 0.0 {
            movement = Some([move_dir.x, move_dir.y]);
        }
    }
    
    // 收集防守方的动作（K键触发技能）
    let action = if keyboard_input.just_pressed(KeyCode::KeyK) {
        // K键随机触发下蹲或侧躲
        if rand::thread_rng().gen_bool(0.5) {
            Some("crouch".to_string())
        } else {
            Some("dodge".to_string())
        }
    } else {
        None
    };
    
    // 获取防守方的位置和动作状态
    let mut defender_pos = None;
    let mut defender_action = "None".to_string();
    for (player_id, transform, dodge_action, role) in player_query.iter() {
        if *player_id == local_player_id && matches!(role, crate::PlayerRole::Defender) {
            let pos = transform.translation;
            defender_pos = Some([pos.x, pos.y, pos.z]);
            defender_action = match dodge_action {
                DodgeAction::None => "None".to_string(),
                DodgeAction::Crouch => "Crouch".to_string(),
                DodgeAction::SideLeft => "SideLeft".to_string(),
                DodgeAction::SideRight => "SideRight".to_string(),
            };
            break;
        }
    }
    
    // 发送防守方状态
    if let Some(pos) = defender_pos {
        let defender_state = NetworkMessage::DefenderState {
            position: pos,
            dodge_action: defender_action.clone(),
        };
        // 检查网络状态
        let remote_addr_ok = network_manager.remote_addr.lock().unwrap().is_some();
        let socket_ok = network_manager.socket.is_some();
        if !remote_addr_ok {
            eprintln!("[防守方] 警告：remote_addr 未设置，无法发送 DefenderState");
        }
        if !socket_ok {
            eprintln!("[防守方] 警告：socket 未设置，无法发送 DefenderState");
        }
        send_network_message(&*network_manager, defender_state);
        // 调试：每60帧打印一次（约1秒）
        if (time.elapsed_seconds() * 60.0) as u32 % 60 == 0 {
            if remote_addr_ok && socket_ok {
                // 调试输出已禁用: println!("[防守方] 发送DefenderState: 位置=({:.1}, {:.1}, {:.1}), 动作={}", pos[0], pos[1], pos[2], defender_action);
            }
        }
    } else {
        // 调试：如果找不到防守方位置
        if (time.elapsed_seconds() * 60.0) as u32 % 60 == 0 {
            // 调试输出已禁用: println!("[防守方] 警告：找不到防守方位置，无法发送DefenderState");
        }
    }
    
    // 发送玩家输入
    let input_msg = NetworkMessage::PlayerInput {
        player_id: local_player_id,
        movement,
        action,
        crosshair_pos: None, // 防守方没有准星
    };
    send_network_message(&*network_manager, input_msg);
}

/// 进攻方：发送准星位置给防守方
/// 基于视图配置（角色），而不是基于玩家身份（房主/客户端）
pub fn sync_crosshair_position_system(
    network_manager: Res<NetworkManager>,
    cursor_pos: Res<CursorPosition>,
    mut sync_timer: Local<f32>,
    time: Res<Time>,
    view_config: Res<ViewConfig>,
    room_info: Res<crate::RoomInfo>,
) {
    // 只有网络模式下才发送准星位置
    if !room_info.is_connected {
        return;
    }
    
    // 基于视图配置（角色），而不是基于玩家身份
    // 只有当前视图是进攻方视图时，才发送准星位置
    if !view_config.is_attacker_view {
        return;
    }
    
    // 每 0.05 秒同步一次
    *sync_timer += time.delta_seconds();
    if *sync_timer < 0.05 {
        return;
    }
    *sync_timer = 0.0;
    
    let crosshair_msg = NetworkMessage::CrosshairPosition {
        position: [cursor_pos.0.x, cursor_pos.0.y],
    };
    // 检查网络状态
    let remote_addr_ok = network_manager.remote_addr.lock().unwrap().is_some();
    let socket_ok = network_manager.socket.is_some();
    if !remote_addr_ok {
        eprintln!("[进攻方] 警告：remote_addr 未设置，无法发送 CrosshairPosition");
    }
    if !socket_ok {
        eprintln!("[进攻方] 警告：socket 未设置，无法发送 CrosshairPosition");
    }
    send_network_message(&*network_manager, crosshair_msg);
    // 调试：每60帧打印一次（约1秒）
    if (time.elapsed_seconds() * 60.0) as u32 % 60 == 0 {
        if remote_addr_ok && socket_ok {
            // 调试输出已禁用: println!("[进攻方] 发送CrosshairPosition: ({:.1}, {:.1})", cursor_pos.0.x, cursor_pos.0.y);
        }
    }
}

/// 进攻方：接收防守方状态（更新防守方位置）
/// 基于视图配置（角色），而不是基于玩家身份（房主/客户端）
pub fn handle_player_input_system(
    network_manager: Res<NetworkManager>,
    mut player_query: Query<(Entity, &crate::PlayerId, &mut Transform, &mut DodgeAction, &mut crate::PlayerRole), (With<crate::PlayerId>, With<crate::PlayerRole>)>,
    mut _round_info: ResMut<RoundInfo>,
    cursor_pos: ResMut<CursorPosition>,
    room_info: Res<crate::RoomInfo>,
    view_config: Res<ViewConfig>,
    time: Res<Time>,
    app_state: Res<State<crate::AppState>>,
) {
    // 只有网络模式才处理
    if !room_info.is_connected {
        return;
    }
    
    // 如果游戏已结束，不允许处理输入
    if *app_state.get() == crate::AppState::GameOver {
        return;
    }
    
    // 基于视图配置（角色），而不是基于玩家身份
    // 只有当前视图是进攻方视图时，才接收防守方状态
    if !view_config.is_attacker_view {
        return;
    }
    
    // 确定本地玩家ID（用于排除本地玩家）
    let local_player_id = if network_manager.is_host {
        crate::PlayerId::Player1
    } else {
        crate::PlayerId::Player2
    };
    
    // 处理接收到的消息（只处理DefenderState，CrosshairPosition由handle_crosshair_position_system处理）
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        let mut messages_to_keep = Vec::new();
        for msg in queue.drain(..) {
            match msg {
                NetworkMessage::DefenderState { position, dodge_action } => {
                    // 更新防守方位置和动作
                    // 防守方状态总是来自对方玩家（不是本地玩家）
                    // 初始：客户端（Player2，防守方）发送给主机，主机更新Player2
                    // 切换后：房主（Player1，防守方）发送给客户端，客户端更新Player1
                    let mut updated = false;
                    for (_entity, pid, mut transform, mut dodge_action_comp, role) in player_query.iter_mut() {
                        // 更新对方玩家的防守方位置（不是本地玩家）
                        if *pid != local_player_id && matches!(*role, crate::PlayerRole::Defender) {
                            transform.translation = Vec3::new(position[0], position[1], position[2]);
                            *dodge_action_comp = match dodge_action.as_str() {
                                "Crouch" => DodgeAction::Crouch,
                                "SideLeft" => DodgeAction::SideLeft,
                                "SideRight" => DodgeAction::SideRight,
                                _ => DodgeAction::None,
                            };
                            updated = true;
                            // println!("[进攻方] 收到DefenderState: 更新玩家 {:?} 位置=({:.1}, {:.1}, {:.1}), 动作={}", 
                            //          pid, position[0], position[1], position[2], dodge_action); // 已禁用：日志太多
                            break;
                        }
                    }
                    if !updated {
                        // 调试输出已禁用: println!("[进攻方] 警告：收到DefenderState但找不到对应的防守方玩家 (local_player_id={:?})", local_player_id);
                }
                }
                _ => {
                    // 其他消息（包括CrosshairPosition）放回队列，由其他系统处理
                    messages_to_keep.push(msg);
                }
            }
        }
        // 将未处理的消息放回队列
        queue.extend(messages_to_keep);
    }
}

/// 防守方：接收准星位置（显示在防守方视角）
/// 基于视图配置（角色），而不是基于玩家身份（房主/客户端）
pub fn handle_crosshair_position_system(
    network_manager: Res<NetworkManager>,
    mut cursor_pos: ResMut<CursorPosition>,
    room_info: Res<crate::RoomInfo>,
    view_config: Res<ViewConfig>,
    time: Res<Time>,
) {
    // 只有网络模式才处理
    if !room_info.is_connected {
        return;
    }
    
    // 基于视图配置（角色），而不是基于玩家身份
    // 只有当前视图是防守方视图时，才接收准星位置
    if view_config.is_attacker_view {
        return;
    }
    
    // 处理接收到的消息（只处理CrosshairPosition）
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        let mut messages_to_keep = Vec::new();
        for msg in queue.drain(..) {
            match msg {
                NetworkMessage::CrosshairPosition { position } => {
                    cursor_pos.0 = Vec2::new(position[0], position[1]);
                    // println!("[防守方] 收到CrosshairPosition: ({:.1}, {:.1})", position[0], position[1]); // 已禁用：日志太多
                }
                _ => {
                    // 其他消息放回队列，由其他系统处理
                    messages_to_keep.push(msg);
                }
            }
        }
        // 将未处理的消息放回队列
        queue.extend(messages_to_keep);
    }
}

/// 处理网络子弹同步（接收方创建子弹）
/// 接收方收到 BulletSpawn 消息后，创建对应的子弹实体
pub fn handle_bullet_spawn_system(
    mut commands: Commands,
    network_manager: Res<NetworkManager>,
    room_info: Res<crate::RoomInfo>,
    mut round_info: ResMut<crate::gameplay::RoundInfo>,
    bullet_query: Query<&crate::gameplay::BulletSyncId, With<crate::gameplay::Bullet>>,
) {
    // 只有网络模式才处理
    if !room_info.is_connected {
        return;
    }
    
    // 处理接收到的消息
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        let mut messages_to_keep = Vec::new();
        for msg in queue.drain(..) {
            match msg {
                NetworkMessage::BulletSpawn {
                    bullet_id,
                    owner,
                    start_pos,
                    target_pos,
                    velocity,
                } => {
                    // 检查是否已经存在相同ID的子弹（避免重复创建）
                    let mut bullet_exists = false;
                    for sync_id in bullet_query.iter() {
                        if sync_id.0 == bullet_id {
                            bullet_exists = true;
                            break;
                        }
                    }
                    
                    if !bullet_exists {
                        // 创建子弹（使用网络消息中的bullet_id）
                        let attacker_pos = Vec2::new(start_pos[0], start_pos[1]);
                        let target = Vec2::new(target_pos[0], target_pos[1]);
                        let vel = Vec2::new(velocity[0], velocity[1]);
                        
                        crate::gameplay::spawn_bullet_with_id(
                            &mut commands,
                            owner,
                            attacker_pos,
                            target,
                            vel,
                            bullet_id,
                        );
                        
                        // 调试输出已禁用: println!("[网络] 收到BulletSpawn: bullet_id={}, owner={:?}, start_pos=({:.1}, {:.1}), target_pos=({:.1}, {:.1})", bullet_id, owner, start_pos[0], start_pos[1], target_pos[0], target_pos[1]);

                        // 更新回合信息（用于主机/客户端同步子弹数量）
                        if round_info.current_attacker == owner {
                            if round_info.bullets_left > 0 {
                                round_info.bullets_left -= 1;
                            }
                            round_info.bullets_fired_this_round += 1;
                        }
                    } else {
                        // 调试输出已禁用: println!("[网络] 警告：收到BulletSpawn但子弹已存在: bullet_id={}", bullet_id);
                    }
                }
                _ => {
                    // 其他消息放回队列
                    messages_to_keep.push(msg);
                }
            }
        }
        // 将未处理的消息放回队列
        queue.extend(messages_to_keep);
    }
}

/// 处理血量更新（接收方更新血量）
/// 接收方收到 HealthUpdate 消息后，更新对应玩家的血量
pub fn handle_health_update_system(
    network_manager: Res<NetworkManager>,
    mut player_query: Query<(&crate::PlayerId, &mut Health), (With<crate::PlayerId>, With<crate::PlayerRole>)>,
    mut round_info: ResMut<RoundInfo>,
    room_info: Res<crate::RoomInfo>,
) {
    // 只有网络模式才处理
    if !room_info.is_connected {
        return;
    }
    
    // 处理接收到的消息
    if let Ok(mut queue) = network_manager.message_queue.lock() {
        let mut messages_to_keep = Vec::new();
        for msg in queue.drain(..) {
            match msg {
                NetworkMessage::HealthUpdate {
                    player_id,
                    health,
                } => {
                    // 更新玩家血量（包括本地玩家和对方玩家）
                    for (pid, mut health_comp) in player_query.iter_mut() {
                        if *pid == player_id {
                            health_comp.0 = health;
                            // 同时更新回合信息中的血量
                            match player_id {
                                crate::PlayerId::Player1 => {
                                    round_info.p1_health = health;
                                }
                                crate::PlayerId::Player2 => {
                                    round_info.p2_health = health;
                                }
                            }
                            // 调试输出已禁用: println!("[网络] 收到HealthUpdate: player_id={:?}, health={:.1}", player_id, health);
                            break;
                        }
                    }
                }
                NetworkMessage::GameOver { winner } => {
                    // 客户端收到游戏结束消息，触发游戏结束状态
                    // 调试输出已禁用: println!("[游戏结束调试] handle_network_messages 收到GameOver消息: winner={:?}", winner);
                    // 这个会在 handle_game_over_network_system 中处理
                    messages_to_keep.push(msg);
                    // 调试输出已禁用: println!("[游戏结束调试] GameOver消息已放回队列（handle_network_messages）");
                }
                NetworkMessage::RematchRequest => {
                    // 房主收到客户端再来一局请求
                    // 调试输出已禁用: println!("[房主] 收到客户端再来一局请求");
                    // 发送准备消息给客户端
                    send_network_message(&*network_manager, NetworkMessage::RematchReady);
                    messages_to_keep.push(msg);
                }
                NetworkMessage::RematchReady => {
                    // 客户端收到房主准备消息
                    // 调试输出已禁用: println!("[客户端] 收到房主再来一局准备消息");
                    messages_to_keep.push(msg);
                }
                _ => {
                    // 其他消息放回队列
                    messages_to_keep.push(msg);
                }
            }
        }
        // 将未处理的消息放回队列
        queue.extend(messages_to_keep);
    }
}
