use addr::parse_domain_name;
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
};
use dialoguer::{theme::ColorfulTheme, Input, Select};
use std::{
    io::{stdout, BufReader, ErrorKind, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};
use terminal_chat::ThreadPool;
const PORT: &str = "10212";
const DEFAULT_ADDRESS: Option<&str> = Some("127.0.0.1");
const CHUNK_SIZE: usize = 256;
#[derive(Debug)]
struct Player {
    id: u32,
    name: String,
    stream: TcpStream,
}
enum PlayerAction {
    Joined,
    SentMessage(String),
    Exited,
}
fn main() {
    main_menu();
}
fn _clear() {
    execute!(stdout(), Clear(ClearType::All)).unwrap();
    print!("\x1B[2J\x1B[1;1H");
    execute!(stdout(), Clear(ClearType::All)).unwrap();
    print!("\x1B[2J\x1B[1;1H");
}
fn clear_line() {
    print!("\x1B[2K");
    print!("\r");
    std::io::stdout().flush().unwrap();
}
fn main_menu() {
    let options = vec!["Host", "Connect to Host"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .items(&options)
        .interact()
        .unwrap();
    match selection {
        0 => {
            match TcpListener::bind(&format!("0.0.0.0:{PORT}")) {
                Ok(listener) => {
                    host(listener);
                }
                Err(err) => {
                    println!("Failed to create TcpListener: {err:#?}");
                    main_menu();
                    std::process::exit(0);
                }
            };
        }
        1 => {
            let ip: String = match DEFAULT_ADDRESS {
                Some(ip) => ip.to_string(),
                None => Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter IP address or domain name")
                    .validate_with(|x: &String| {
                        match x.parse::<Ipv4Addr>() {
                            Ok(_) => return Ok(()),
                            Err(_) => match parse_domain_name(x) {
                                Ok(name) => {
                                    if name.to_string().contains(".") {
                                        return Ok(());
                                    } else {
                                        return Err("Invalid domain name".to_owned());
                                    }
                                }
                                Err(_) => {
                                    return Err(
                                        "Couldn't parse domain name or ip address".to_owned()
                                    )
                                }
                            },
                        };
                    })
                    .interact()
                    .unwrap(),
            };
            let ip = format!("{ip}:{PORT}")
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap();
            match TcpStream::connect(ip) {
                Ok(stream) => {
                    client(stream);
                }
                Err(err) => {
                    println!("Failed to connect to tcpstream: {err:#?}");
                    main_menu();
                    std::process::exit(0);
                }
            };
        }
        _ => {}
    };
}
fn host(listener: TcpListener) {
    let players: Vec<Player> = vec![];
    let players_mutex = Arc::new(Mutex::new(players));
    let pool = ThreadPool::new(10);
    let players_mutex1 = Arc::clone(&players_mutex);
    let mut count = 0;
    let (tx, rx): (
        Sender<(u32, Option<String>, PlayerAction)>,
        Receiver<(u32, Option<String>, PlayerAction)>,
    ) = channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            let players_mutex2 = Arc::clone(&players_mutex1);
            let tx1 = tx.clone();
            pool.execute(move || {
                handle_connection(stream, Arc::clone(&players_mutex2), count, tx1.clone());
            });
            count += 1;
        }
    });
    while let Ok(received) = rx.recv() {
        if let Some(sender_name) = received.1 {
            match received.2 {
                PlayerAction::Joined => {
                    send_to_players(
                        received.0,
                        format!("{} has joined the server!", sender_name),
                        Arc::clone(&players_mutex),
                    );
                    let player_amount = players_mutex.lock().unwrap().len();
                    let join_message: String;
                    if player_amount > 1 {
                        join_message = format!("Joined server with {player_amount} people!");
                    } else {
                        join_message = format!("Oh no! It looks like you're the only one here... Don't worry, i'm sure more people will join soon!");
                    }
                    for player in players_mutex.lock().unwrap().iter_mut() {
                        if player.id == received.0 {
                            send_message(&mut player.stream, join_message).unwrap();
                            break;
                        }
                    }
                }
                PlayerAction::SentMessage(message) => {
                    send_to_players(
                        received.0,
                        format!("{}: {}", sender_name, message),
                        Arc::clone(&players_mutex),
                    );
                }
                PlayerAction::Exited => {
                    send_to_players(
                        received.0,
                        format!("{} has left the server :(", sender_name),
                        Arc::clone(&players_mutex),
                    );
                }
            }
        }
    }
}
fn send_to_players(id: u32, message: String, players: Arc<Mutex<Vec<Player>>>) {
    for player in players.lock().unwrap().iter_mut() {
        if id != player.id {
            send_message(&mut player.stream, message.clone()).unwrap();
        }
    }
}
fn handle_connection(
    mut stream: TcpStream,
    players: Arc<Mutex<Vec<Player>>>,
    id: u32,
    tx: Sender<(u32, Option<String>, PlayerAction)>,
) {
    let mut first_time = true;
    let mut name: Option<String> = None;
    send_players(&mut stream, Arc::clone(&players), id);
    let (string_tx, string_rx): (
        Sender<Result<String, std::io::Error>>,
        Receiver<Result<String, std::io::Error>>,
    ) = channel();
    let stream_clone = stream.try_clone().unwrap();
    thread::spawn(|| {
        get_stream_data(stream_clone, string_tx);
    });
    while let Ok(message) = string_rx.recv() {
        let message = match message {
            Ok(message) => message,
            Err(_) => {
                let mut players = players.lock().unwrap();
                for i in 0..players.len() {
                    if players[i].id == id {
                        players.remove(i);
                        break;
                    }
                }
                tx.send((id, name.clone(), PlayerAction::Exited)).unwrap();
                break;
            }
        };
        if first_time {
            name = Some(message.clone());
            let player = Player {
                id,
                name: message.clone(),
                stream: stream.try_clone().unwrap(),
            };
            players.lock().unwrap().push(player);
            first_time = false;
            tx.send((id, name.clone(), PlayerAction::Joined)).unwrap();
        } else {
            tx.send((id, name.clone(), PlayerAction::SentMessage(message)))
                .unwrap();
        }
    }
}

fn send_players(stream: &mut TcpStream, players: Arc<Mutex<Vec<Player>>>, id: u32) {
    let mut player_names = String::from("");
    for player in players.lock().unwrap().iter() {
        if player.id != id {
            player_names = format!("{player_names} {}", player.name);
        }
    }
    send_message(stream, player_names).unwrap();
}
fn client(stream: TcpStream) {
    let (tx, rx): (
        Sender<Result<String, std::io::Error>>,
        Receiver<Result<String, std::io::Error>>,
    ) = channel();
    let stream_clone: TcpStream = stream.try_clone().unwrap();
    thread::spawn(move || {
        get_stream_data(stream_clone, tx);
    });
    let mut first_message = true;
    let mut name: Option<String> = None;
    let stream_clone: TcpStream = stream.try_clone().unwrap();
    while let Ok(message) = rx.recv() {
        let message = match message {
            Ok(str) => str,
            Err(err) => {
                handle_client_errors(err);
                continue;
            }
        };
        if first_message {
            let player_names = message;
            first_message = false;
            name = Some(
                Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter name: ")
                    .validate_with(|x: &String| {
                        if player_names.contains(x) {
                            return Err("Name is already taken");
                        }
                        if x.contains("#") {
                            Err("Name cannot contain hashtags")
                        } else {
                            Ok(())
                        }
                    })
                    .interact()
                    .unwrap(),
            );
            let mut stream_clone1: TcpStream = stream_clone.try_clone().unwrap();
            let name_clone = name.clone();
            if let Err(err) = send_message(&mut stream_clone1, name_clone.unwrap()) {
                handle_client_errors(err);
            }
            let stream_clone1: TcpStream = stream_clone.try_clone().unwrap();
            let name_clone = name.clone();
            thread::spawn(move || {
                get_client_input(name_clone.unwrap(), stream_clone1);
            });
            continue;
        }
        let name_clone = name.clone();
        if let Some(name) = name_clone.clone() {
            clear_line();
            print!("{message}\n{name}: ");
            std::io::stdout().flush().unwrap();
        }
    }
}
fn send_message(stream: &mut TcpStream, message: String) -> Result<(), std::io::Error> {
    let message_size = message.len() as u32;
    let message_size_bytes = split_u32_into_u8s(message_size);
    let mut message_bytes: Vec<u8> = vec![];
    for byte in message_size_bytes {
        message_bytes.push(byte);
    }
    message_bytes.extend_from_slice(message.as_bytes());
    stream.write_all(&message_bytes)?;
    Ok(())
}
fn get_client_input(name: String, mut stream: TcpStream) {
    loop {
        let mut str = String::from("");
        print!("{name}: ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut str).unwrap();
        str = str.trim().to_string();
        send_message(&mut stream, str).unwrap();
    }
}
fn get_stream_data(mut stream: TcpStream, tx: Sender<Result<String, std::io::Error>>) {
    let mut reader = BufReader::new(&mut stream);
    let mut buffer: Vec<u8> = vec![];
    let mut bytes_left = 0;
    let mut string_in_progress = false;
    loop {
        let mut chunk = [0u8; CHUNK_SIZE];
        match reader.read(&mut chunk) {
            Err(err) => {
                tx.send(Err(err)).unwrap();
                return;
            }
            Ok(bytes) => {
                //println!("Got {bytes} bytes!");
                if string_in_progress {
                    //println!("Buffer before: {buffer:?}");
                    buffer.extend_from_slice(&chunk[..bytes]);
                    //println!("Buffer after: {buffer:?}");
                    //println!("Bytes left before: {bytes_left}");
                    bytes_left = (bytes_left - bytes as i32).max(0);
                    //println!("Bytes left after: {bytes_left}");
                } else {
                    //println!("Chunk: {chunk:?}");
                    let (left, right) = chunk.split_at(4);
                    //println!("Left: {left:?}");
                    //println!("Right: {right:?}");
                    let mut chunk_length_arr = [0u8; 4];
                    for i in 0..4 {
                        chunk_length_arr[i] = left[i];
                    }
                    let mut chunk = [0u8; CHUNK_SIZE - 4];
                    for i in 0..CHUNK_SIZE - 4 {
                        chunk[i] = right[i];
                    }
                    bytes_left = combine_u8s_into_u32(chunk_length_arr) as i32;
                    //println!("Buffer before: {buffer:?}");
                    buffer.extend_from_slice(&chunk[..(bytes - 4)]);
                    //println!("Buffer after: {buffer:?}");
                    //println!("Bytes left before: {bytes_left}");
                    bytes_left = (bytes_left - (bytes as i32 - 4).max(0)).max(0);
                    //println!("Bytes left after: {bytes_left}");
                    string_in_progress = true;
                }
                if bytes_left == 0 {
                    string_in_progress = false;
                    tx.send(Ok(String::from_utf8(buffer).unwrap())).unwrap();
                    //println!("Sent string");
                    buffer = vec![];
                }
            }
        }
    }
}
fn handle_client_errors(err: std::io::Error) {
    clear_line();
    match err.kind() {
        ErrorKind::ConnectionReset => {
            println!("Server closed");
        }
        _ => {
            println!("Exiting because: {err:?}");
        }
    }
    std::process::exit(0);
}
fn split_u32_into_u8s(input: u32) -> [u8; 4] {
    let byte1 = (input >> 24) as u8;
    let byte2 = ((input >> 16) & 0xFF) as u8;
    let byte3 = ((input >> 8) & 0xFF) as u8;
    let byte4 = (input & 0xFF) as u8;
    [byte1, byte2, byte3, byte4]
}

fn combine_u8s_into_u32(bytes: [u8; 4]) -> u32 {
    let byte1 = (bytes[0] as u32) << 24;
    let byte2 = (bytes[1] as u32) << 16;
    let byte3 = (bytes[2] as u32) << 8;
    let byte4 = bytes[3] as u32;
    byte1 | byte2 | byte3 | byte4
}
