use std::io::BufReader;
use std::thread;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
    Duration,
};
use std::sync::{
    Arc,
    Mutex,
    RwLock,
    mpsc::{
        self,
        Sender,
        Receiver,
    },
    atomic::AtomicBool,
};
use std::net::{
    TcpListener,
    TcpStream,
};
use mcproto_rs::v1_16_3::{
    RawPacket753,
    Packet753,
    LoginEncryptionRequestSpec,
    LoginSuccessSpec,
    PlayJoinGameSpec,
    GameMode,
    PreviousGameMode,
    PlayServerKeepAliveSpec,
};
use mcproto_rs::types::{
    Chat,
    BaseComponent,
    TextComponent,
    CountedArray,
    VarInt,
    NamedNbtTag,
};
use mcproto_rs::nbt::{
    NamedTag,
    Tag,
};
use mcproto_rs::uuid::UUID4;
use mcproto_rs::protocol::{
    RawPacket,
    PacketDirection,
    State,
};
use craftio_rs::{
    CraftConnection,
    CraftIo,
    CraftReader,
    CraftWriter,
    CraftSyncReader,
    CraftSyncWriter,
};

fn main() {
    let addr = "127.0.0.1:25565";
    let tcp_listener = TcpListener::bind(addr).unwrap();
    println!("listening on {}", addr);
    for stream in tcp_listener.incoming() {
        if let Ok(stream) = stream {
            let peeraddr = stream.peer_addr().unwrap();
            println!("got connection {}", peeraddr);
            let mut conn = CraftConnection::from_std_with_state(stream, PacketDirection::ServerBound, State::Handshaking).unwrap();
            thread::spawn(move || {
                // receive handshake
                if let Ok(Some(Packet753::Handshake(handshake))) = conn.read_packet::<RawPacket753>() {
                    println!("got handshake from {}", peeraddr);
                    conn.set_state(State::Login);
                }
                else {
                    println!("invalid handshake from {}", peeraddr);
                    return;
                }
                // receive player login
                if let Ok(Some(Packet753::LoginStart(logindata))) = conn.read_packet::<RawPacket753>() {
                    println!("player name: {}", logindata.name);
                    // let pubkey = &mut [0u8; 16]
                    // conn.write_packet(Packet753::LoginEncryptionRequest(LoginEncryptionRequestSpec{
                    //     server_id: "                ".into(), // ' '*16
                    //     public_key: //
                    // })).unwrap();
                    conn.write_packet(Packet753::LoginSuccess(LoginSuccessSpec {
                        uuid: UUID4::random(),
                        username: logindata.name,
                    })).unwrap();
                }
                else {
                    println!("invalid login start from {}", peeraddr);
                    return;
                }
                // play state
                conn.set_state(State::Play);
                // send join game
                let dimension_tag = NamedNbtTag { root: NamedTag {
                    name: "lamo".into(),
                    payload: Tag::End,
                }};
                conn.write_packet(Packet753::PlayJoinGame(PlayJoinGameSpec {
                    entity_id: 0,
                    is_hardcore: false,
                    gamemode: GameMode::Spectator,
                    previous_gamemode: PreviousGameMode::NoPrevious,
                    worlds: CountedArray::from(vec!["lmao".into()]),
                    dimension_codec: dimension_tag.clone(),
                    dimension: dimension_tag,
                    hashed_seed: 0,
                    max_players: 0.into(),
                    view_distance: 0.into(),
                    world_name: "lmoa".into(),
                    reduced_debug_info: false,
                    enable_respawn_screen: false,
                    is_debug: false,
                    is_flat: true,
                })).unwrap();
                // receive client brand
                // receive client settings
                // send player slot
                // send recipes
                // send tags
                let (mut reader, writer) = conn.into_split();
                let writer_arc = Arc::new(Mutex::new(writer));
                thread::spawn(move || keep_alive_loop(writer_arc.clone()));
                loop {
                    let packet = reader.read_raw_packet::<RawPacket753>();
                    match packet {
                        Ok(None) => {
                            println!("none packet? from {}", peeraddr);
                            break;
                        }, // i think this behavior is when stream EOF
                        Ok(Some(packet)) => match packet {
                            RawPacket753::PlayClientKeepAlive(body) => { // this is 0x10
                                println!("got alive message from client {}", peeraddr);
                            },
                            _ => {},
                        },
                        Err(e) => eprintln!("packet error {}", e),
                    }
                }
            });
        }
    }
}

fn time_since_epoch() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn keep_alive_loop(writer: Arc<Mutex<CraftWriter<TcpStream>>>) {
    loop {
        thread::sleep_ms(5000);
        writer.lock().unwrap().write_packet(
            Packet753::PlayServerKeepAlive(PlayServerKeepAliveSpec { id: time_since_epoch() as i64 })
        ).unwrap();
    }
}
